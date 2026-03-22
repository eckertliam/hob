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
            while let Some(newline_pos) = buf.find('\n') {
                let line = buf[..newline_pos].trim_end_matches('\r').to_string();
                buf = buf[newline_pos + 1..].to_string();

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
                    current_event = Some(value.trim().to_string());
                } else if let Some(value) = line.strip_prefix("data:") {
                    if !current_data.is_empty() {
                        current_data.push('\n');
                    }
                    current_data.push_str(value.trim());
                }
                // Ignore other fields (id:, retry:, comments)
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
