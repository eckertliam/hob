//! Provider abstraction layer.
//!
//! Defines a uniform streaming interface over different LLM APIs.
//! Each provider (Anthropic, OpenAI, etc.) normalizes its SSE format
//! into a common `StreamEvent` enum.

pub mod anthropic;
pub mod sse;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// A normalized streaming event. Both Anthropic and OpenAI SSE formats
/// map onto this enum.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A text content block has started.
    TextStart { index: u32 },
    /// A chunk of text content.
    TextDelta { index: u32, text: String },
    /// A text content block has ended.
    TextStop { index: u32 },
    /// A tool use content block has started.
    ToolStart {
        index: u32,
        id: String,
        name: String,
    },
    /// A chunk of tool input JSON.
    ToolDelta { index: u32, args_json: String },
    /// A tool use content block has ended.
    ToolStop { index: u32 },
    /// End-of-message metadata (stop reason, token usage).
    MessageDelta {
        stop_reason: Option<StopReason>,
        usage: Option<Usage>,
    },
    /// The stream is complete.
    MessageStop,
}

/// Why the model stopped generating.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Natural completion.
    EndTurn,
    /// Model wants to call tools.
    ToolUse,
    /// Hit max_tokens limit.
    MaxTokens,
    /// Hit a stop sequence.
    StopSequence,
    /// Content filter triggered.
    ContentFilter,
}

/// Token usage information.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A message in the conversation history, provider-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    User { content: Vec<ContentBlock> },
    Assistant { content: Vec<ContentBlock> },
}

/// A content block within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text.
    Text { text: String },
    /// A tool invocation (in assistant messages).
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// A tool result (in user messages).
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// Tool definition sent to the model.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Parameters for a streaming request.
pub struct StreamRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
}

/// Trait that all LLM providers implement.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Start a streaming request. Returns a channel that yields normalized events.
    async fn stream(
        &self,
        request: StreamRequest,
    ) -> Result<mpsc::Receiver<Result<StreamEvent>>>;
}
