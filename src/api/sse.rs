//! Shared SSE line parser.
//!
//! Parses a raw byte stream into (event_type, data) pairs.
//! Both Anthropic and OpenAI use SSE, but with different event payloads.

use anyhow::Result;
use futures::StreamExt;
use reqwest::Response;
use tokio::sync::mpsc;

/// A parsed SSE event: the event type and the data payload.
#[derive(Debug)]
pub struct SseEvent {
    /// The `event:` field, if present. None for OpenAI (which omits it).
    pub event: Option<String>,
    /// The `data:` field content.
    pub data: String,
}

/// Strip the SSE field value: per spec, only strip a single leading space
/// after the colon, not all whitespace.
fn strip_field_value(value: &str) -> &str {
    value.strip_prefix(' ').unwrap_or(value)
}

/// Consume a streaming HTTP response and parse SSE events into a channel.
///
/// SSE format:
///   event: <type>\n
///   data: <json>\n
///   \n
///
/// OpenAI omits the `event:` line and uses `data: [DONE]` as sentinel.
/// Anthropic includes `event:` lines and uses `event: message_stop`.
pub fn parse_stream(response: Response) -> mpsc::Receiver<Result<SseEvent>> {
    let (tx, rx) = mpsc::channel(64);

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut buf = String::new();
        let mut current_event: Option<String> = None;
        let mut current_data = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(Err(anyhow::anyhow!("stream error: {e}"))).await;
                    return;
                }
            };

            buf.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete lines
            loop {
                let Some(newline_pos) = buf.find('\n') else {
                    break;
                };
                let line_end = if newline_pos > 0 && buf.as_bytes()[newline_pos - 1] == b'\r' {
                    newline_pos - 1
                } else {
                    newline_pos
                };
                // Borrow the line content before mutating buf
                let line = &buf[..line_end];

                if line.is_empty() {
                    // Empty line = end of event
                    if !current_data.is_empty() {
                        let event = SseEvent {
                            event: current_event.take(),
                            data: std::mem::take(&mut current_data),
                        };
                        if tx.send(Ok(event)).await.is_err() {
                            return; // receiver dropped
                        }
                    }
                    current_event = None;
                } else if let Some(value) = line.strip_prefix("event:") {
                    current_event = Some(strip_field_value(value).to_string());
                } else if let Some(value) = line.strip_prefix("data:") {
                    if !current_data.is_empty() {
                        current_data.push('\n');
                    }
                    current_data.push_str(strip_field_value(value));
                }
                // Ignore other fields (id:, retry:, comments starting with :)

                // Drain the consumed line from buf without reallocating
                // when there's more data remaining
                buf.drain(..newline_pos + 1);
            }
        }

        // Flush any remaining event
        if !current_data.is_empty() {
            let event = SseEvent {
                event: current_event.take(),
                data: current_data,
            };
            let _ = tx.send(Ok(event)).await;
        }
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a mock reqwest::Response from raw SSE bytes.
    fn mock_response(body: &'static [u8]) -> Response {
        let http_resp: http::Response<Vec<u8>> = http::Response::builder()
            .status(200)
            .body(body.to_vec())
            .unwrap();
        http_resp.into()
    }

    #[tokio::test]
    async fn test_parse_anthropic_text_stream() {
        let sse = b"\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-20250514\",\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

        let response = mock_response(sse);
        let mut rx = parse_stream(response);
        let mut events = vec![];
        while let Some(Ok(event)) = rx.recv().await {
            events.push(event);
        }

        assert_eq!(events.len(), 7);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_start"));
        assert_eq!(events[2].event.as_deref(), Some("content_block_delta"));
        assert!(events[2].data.contains("Hello"));
        assert_eq!(events[3].event.as_deref(), Some("content_block_delta"));
        assert!(events[3].data.contains(" world"));
        assert_eq!(events[4].event.as_deref(), Some("content_block_stop"));
        assert_eq!(events[5].event.as_deref(), Some("message_delta"));
        assert!(events[5].data.contains("end_turn"));
        assert_eq!(events[6].event.as_deref(), Some("message_stop"));
    }

    #[tokio::test]
    async fn test_parse_openai_style_no_event_field() {
        let sse = b"\
data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\
\n\
data: [DONE]\n\
\n";

        let response = mock_response(sse);
        let mut rx = parse_stream(response);
        let mut events = vec![];
        while let Some(Ok(event)) = rx.recv().await {
            events.push(event);
        }

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, None);
        assert!(events[0].data.contains("Hi"));
        assert_eq!(events[1].event, None);
        assert_eq!(events[1].data, "[DONE]");
    }

    #[tokio::test]
    async fn test_strip_field_value_single_space() {
        // Per SSE spec: only strip one leading space after the colon
        assert_eq!(strip_field_value(" hello"), "hello");
        assert_eq!(strip_field_value("  two spaces"), " two spaces");
        assert_eq!(strip_field_value("no space"), "no space");
        assert_eq!(strip_field_value(""), "");
    }

    #[tokio::test]
    async fn test_parse_tool_use_stream() {
        let sse = b"\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"read_file\",\"input\":{}}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"foo.rs\\\"}\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":1}\n\
\n";

        let response = mock_response(sse);
        let mut rx = parse_stream(response);
        let mut events = vec![];
        while let Some(Ok(event)) = rx.recv().await {
            events.push(event);
        }

        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event.as_deref(), Some("content_block_start"));
        assert!(events[0].data.contains("tool_use"));
        assert!(events[0].data.contains("read_file"));
    }
}
