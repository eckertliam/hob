//! Anthropic Messages API provider.
//!
//! Converts Anthropic SSE events into the common StreamEvent format.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc;

use super::sse;
use super::{
    ContentBlock, Message, Provider, StopReason, StreamEvent, StreamRequest, ToolDef, Usage,
};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    api_key: String,
    http: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::Client::new(),
        }
    }
}

// --- Request body types (Anthropic wire format) ---

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    stream: bool,
    system: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ApiTool>,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: serde_json::Value,
}

#[derive(Serialize)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

/// Convert our provider-agnostic messages into Anthropic wire format.
fn convert_messages(messages: &[Message]) -> Vec<ApiMessage> {
    messages
        .iter()
        .map(|msg| match msg {
            Message::User { content } => ApiMessage {
                role: "user".into(),
                content: serde_json::to_value(content).unwrap_or_default(),
            },
            Message::Assistant { content } => ApiMessage {
                role: "assistant".into(),
                content: serde_json::to_value(content).unwrap_or_default(),
            },
        })
        .collect()
}

fn convert_tools(tools: &[ToolDef]) -> Vec<ApiTool> {
    tools
        .iter()
        .map(|t| ApiTool {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
        })
        .collect()
}

/// Parse Anthropic's stop_reason string.
fn parse_stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        _ => StopReason::EndTurn,
    }
}

/// Tracks which content block indices are text vs tool_use so we can
/// emit the correct stop event.
#[derive(Default)]
struct BlockTracker {
    /// Maps content block index to true=tool, false=text.
    types: std::collections::HashMap<u32, bool>,
}

impl BlockTracker {
    fn register_text(&mut self, index: u32) {
        self.types.insert(index, false);
    }

    fn register_tool(&mut self, index: u32) {
        self.types.insert(index, true);
    }

    fn is_tool(&self, index: u32) -> bool {
        self.types.get(&index).copied().unwrap_or(false)
    }
}

/// Parse a single Anthropic SSE event into zero or more StreamEvents.
fn parse_event(
    event_type: &str,
    data: &serde_json::Value,
    tracker: &mut BlockTracker,
) -> Vec<StreamEvent> {
    match event_type {
        "message_start" => {
            // Extract usage from message_start if present
            let usage = data
                .pointer("/message/usage")
                .and_then(|u| {
                    Some(Usage {
                        input_tokens: u.get("input_tokens")?.as_u64()? as u32,
                        output_tokens: u.get("output_tokens")?.as_u64()? as u32,
                    })
                });
            if let Some(usage) = usage {
                vec![StreamEvent::MessageDelta {
                    stop_reason: None,
                    usage: Some(usage),
                }]
            } else {
                vec![]
            }
        }
        "content_block_start" => {
            let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let block = data.get("content_block").unwrap_or(data);
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match block_type {
                "text" => {
                    tracker.register_text(index);
                    vec![StreamEvent::TextStart { index }]
                }
                "tool_use" => {
                    tracker.register_tool(index);
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![StreamEvent::ToolStart { index, id, name }]
                }
                _ => vec![],
            }
        }
        "content_block_delta" => {
            let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let delta = data.get("delta").unwrap_or(data);
            let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match delta_type {
                "text_delta" => {
                    let text = delta
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![StreamEvent::TextDelta { index, text }]
                }
                "input_json_delta" => {
                    let partial = delta
                        .get("partial_json")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![StreamEvent::ToolDelta {
                        index,
                        args_json: partial,
                    }]
                }
                _ => vec![],
            }
        }
        "content_block_stop" => {
            let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if tracker.is_tool(index) {
                vec![StreamEvent::ToolStop { index }]
            } else {
                vec![StreamEvent::TextStop { index }]
            }
        }
        "message_delta" => {
            let delta = data.get("delta").unwrap_or(data);
            let stop_reason = delta
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(parse_stop_reason);
            let usage = data.pointer("/usage").and_then(|u| {
                Some(Usage {
                    input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                        as u32,
                    output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                        as u32,
                })
            });
            vec![StreamEvent::MessageDelta { stop_reason, usage }]
        }
        "message_stop" => vec![StreamEvent::MessageStop],
        "ping" => vec![],
        _ => {
            tracing::debug!("unknown anthropic event: {event_type}");
            vec![]
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn stream(
        &self,
        request: StreamRequest,
    ) -> Result<mpsc::Receiver<Result<StreamEvent>>> {
        let body = ApiRequest {
            model: request.model,
            max_tokens: request.max_tokens,
            stream: true,
            system: request.system,
            messages: convert_messages(&request.messages),
            tools: convert_tools(&request.tools),
        };

        let response = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("failed to connect to Anthropic API")?;

        // Check for HTTP errors before trying to parse SSE
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error {status}: {body}");
        }

        let mut sse_rx = sse::parse_stream(response);
        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            let mut tracker = BlockTracker::default();

            while let Some(sse_result) = sse_rx.recv().await {
                match sse_result {
                    Ok(sse_event) => {
                        let event_type = sse_event.event.as_deref().unwrap_or("unknown");

                        let data: serde_json::Value = match serde_json::from_str(&sse_event.data) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::error!("failed to parse SSE data: {e}");
                                continue;
                            }
                        };

                        for stream_event in parse_event(event_type, &data, &mut tracker) {
                            if tx.send(Ok(stream_event)).await.is_err() {
                                return; // receiver dropped
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }
}
