//! OpenAI Chat Completions API provider.
//!
//! Converts OpenAI SSE chunks into the common StreamEvent format.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc;

use super::sse;
use super::{
    ContentBlock, Message, Provider, StopReason, StreamEvent, StreamRequest, ToolDef, Usage,
};

const DEFAULT_API_URL: &str = "https://api.openai.com/v1/chat/completions";

pub struct OpenAIProvider {
    api_key: String,
    api_url: String,
    http: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            api_url: DEFAULT_API_URL.to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Create with a custom base URL (for OpenAI-compatible APIs).
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            api_url: format!("{}/v1/chat/completions", base_url.trim_end_matches('/')),
            http: reqwest::Client::new(),
        }
    }
}

// --- Request body types (OpenAI wire format) ---

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    messages: Vec<ApiMessage>,
    stream: bool,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ApiTool>,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ApiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Serialize)]
struct ApiToolCall {
    id: String,
    r#type: String,
    function: ApiFunction,
}

#[derive(Serialize)]
struct ApiFunction {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct ApiTool {
    r#type: String,
    function: ApiToolFunction,
}

#[derive(Serialize)]
struct ApiToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

fn convert_messages(messages: &[Message]) -> Result<Vec<ApiMessage>> {
    let mut result = Vec::new();

    for msg in messages {
        match msg {
            Message::User { content } => {
                // Check if this is a tool result message
                let tool_results: Vec<&ContentBlock> = content
                    .iter()
                    .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                    .collect();

                if !tool_results.is_empty() {
                    // OpenAI uses separate "tool" role messages for each result
                    for block in tool_results {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = block
                        {
                            result.push(ApiMessage {
                                role: "tool".into(),
                                content: Some(serde_json::Value::String(content.clone())),
                                tool_calls: None,
                                tool_call_id: Some(tool_use_id.clone()),
                                name: None,
                            });
                        }
                    }
                } else {
                    // Regular user message — combine text blocks
                    let text: String = content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    result.push(ApiMessage {
                        role: "user".into(),
                        content: Some(serde_json::Value::String(text)),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
            }
            Message::Assistant { content } => {
                let text: String = content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                let tool_calls: Vec<ApiToolCall> = content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolUse { id, name, input } => Some(ApiToolCall {
                            id: id.clone(),
                            r#type: "function".into(),
                            function: ApiFunction {
                                name: name.clone(),
                                arguments: serde_json::to_string(input).unwrap_or_default(),
                            },
                        }),
                        _ => None,
                    })
                    .collect();

                result.push(ApiMessage {
                    role: "assistant".into(),
                    content: if text.is_empty() {
                        None
                    } else {
                        Some(serde_json::Value::String(text))
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                    name: None,
                });
            }
        }
    }

    Ok(result)
}

fn convert_tools(tools: &[ToolDef]) -> Vec<ApiTool> {
    tools
        .iter()
        .map(|t| ApiTool {
            r#type: "function".into(),
            function: ApiToolFunction {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
            },
        })
        .collect()
}

/// Parse an OpenAI SSE chunk into StreamEvents.
///
/// OpenAI chunks look like:
///   data: {"choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}
///   data: [DONE]
fn parse_chunk(data: &str) -> Vec<StreamEvent> {
    if data == "[DONE]" {
        return vec![StreamEvent::MessageStop];
    }

    let v: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let choice = match v.pointer("/choices/0") {
        Some(c) => c,
        None => return vec![],
    };

    let mut events = Vec::new();
    let delta = choice.get("delta").unwrap_or(choice);

    // Text content
    if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            events.push(StreamEvent::TextDelta {
                index: 0,
                text: text.to_string(),
            });
        }
    }

    // Tool calls
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tool_calls {
            let index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

            // First chunk has id and function name
            if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                let name = tc
                    .pointer("/function/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                events.push(StreamEvent::ToolStart {
                    index,
                    id: id.to_string(),
                    name,
                });
            }

            // Argument deltas
            if let Some(args) = tc.pointer("/function/arguments").and_then(|v| v.as_str()) {
                if !args.is_empty() {
                    events.push(StreamEvent::ToolDelta {
                        index,
                        args_json: args.to_string(),
                    });
                }
            }
        }
    }

    // Finish reason
    if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
        let stop_reason = match reason {
            "stop" => Some(StopReason::EndTurn),
            "tool_calls" => Some(StopReason::ToolUse),
            "length" => Some(StopReason::MaxTokens),
            "content_filter" => Some(StopReason::ContentFilter),
            _ => None,
        };

        // Extract usage if present
        let usage = v.get("usage").and_then(|u| {
            Some(Usage {
                input_tokens: u.get("prompt_tokens")?.as_u64()? as u32,
                output_tokens: u.get("completion_tokens")?.as_u64()? as u32,
            })
        });

        events.push(StreamEvent::MessageDelta {
            stop_reason,
            usage,
        });
    }

    events
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn stream(
        &self,
        request: StreamRequest,
    ) -> Result<mpsc::Receiver<Result<StreamEvent>>> {
        let mut api_messages = vec![ApiMessage {
            role: "system".into(),
            content: Some(serde_json::Value::String(request.system)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];
        api_messages.extend(convert_messages(&request.messages)?);

        let body = ApiRequest {
            model: request.model,
            messages: api_messages,
            stream: true,
            max_tokens: request.max_tokens,
            tools: convert_tools(&request.tools),
        };

        let response = self
            .http
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("failed to connect to OpenAI API")?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(crate::error::parse_retry_after);
            let body = response.text().await.unwrap_or_default();
            let mut classified = crate::error::classify(status.as_u16(), &body);
            classified.retry_after = retry_after;
            return Err(classified.into());
        }

        let mut sse_rx = sse::parse_stream(response);
        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            while let Some(sse_result) = sse_rx.recv().await {
                match sse_result {
                    Ok(sse_event) => {
                        for stream_event in parse_chunk(&sse_event.data) {
                            if tx.send(Ok(stream_event)).await.is_err() {
                                return;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chunk_text_delta() {
        let events = parse_chunk(
            r#"{"choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::TextDelta { text, .. } => assert_eq!(text, "Hello"),
            _ => panic!("expected TextDelta"),
        }
    }

    #[test]
    fn test_parse_chunk_done() {
        let events = parse_chunk("[DONE]");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::MessageStop));
    }

    #[test]
    fn test_parse_chunk_finish_reason_stop() {
        let events = parse_chunk(
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        );
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::MessageDelta {
                stop_reason: Some(StopReason::EndTurn),
                ..
            }
        )));
    }

    #[test]
    fn test_parse_chunk_tool_call_start() {
        let events = parse_chunk(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_123","type":"function","function":{"name":"read_file","arguments":""}}]},"finish_reason":null}]}"#,
        );
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::ToolStart { name, .. } if name == "read_file"
        )));
    }

    #[test]
    fn test_parse_chunk_tool_call_args() {
        let events = parse_chunk(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]},"finish_reason":null}]}"#,
        );
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::ToolDelta { args_json, .. } if args_json.contains("path")
        )));
    }

    #[test]
    fn test_convert_messages_user_text() {
        let msgs = vec![Message::User {
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
        }];
        let result = convert_messages(&msgs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
    }

    #[test]
    fn test_convert_messages_tool_result() {
        let msgs = vec![Message::User {
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_123".into(),
                content: "file contents".into(),
                is_error: false,
            }],
        }];
        let result = convert_messages(&msgs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "tool");
        assert_eq!(result[0].tool_call_id.as_deref(), Some("call_123"));
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![ToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let result = convert_tools(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].r#type, "function");
        assert_eq!(result[0].function.name, "read_file");
    }
}
