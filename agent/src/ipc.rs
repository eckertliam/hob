//! Newline-delimited JSON IPC over stdin/stdout.
//!
//! Message format: one JSON object per line.
//! Incoming (from Emacs): Request
//! Outgoing (to Emacs): Response

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Messages received from Emacs.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Start an agent task with a prompt.
    Task { id: String, prompt: String },
    /// Cancel an in-progress task.
    Cancel { id: String },
    /// Ping for health check.
    Ping,
}

/// Messages sent to Emacs.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Streaming token from the model.
    Token { id: String, content: String },
    /// A tool is being invoked.
    ToolCall { id: String, tool: String, input: serde_json::Value },
    /// A tool has completed.
    ToolResult { id: String, tool: String, output: String },
    /// Task completed successfully.
    Done { id: String },
    /// An error occurred.
    Error { id: String, message: String },
    /// Response to Ping.
    Pong,
}

/// Write a response to stdout as a newline-delimited JSON line.
pub async fn send(response: &Response) -> Result<()> {
    let mut stdout = tokio::io::stdout();
    let mut line = serde_json::to_string(response)?;
    line.push('\n');
    stdout.write_all(line.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}

/// Main IPC loop: read requests from stdin, dispatch to agent.
pub async fn run_loop() -> Result<()> {
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                handle_request(request).await?;
            }
            Err(e) => {
                tracing::error!("Failed to parse request: {e}: {line}");
            }
        }
    }

    Ok(())
}

async fn handle_request(request: Request) -> Result<()> {
    match request {
        Request::Ping => {
            send(&Response::Pong).await?;
        }
        Request::Task { id, prompt: _ } => {
            // TODO: delegate to agent::run_task(id, prompt)
            send(&Response::Error {
                id,
                message: "not implemented".into(),
            })
            .await?;
        }
        Request::Cancel { id } => {
            // TODO: cancel in-flight task by id
            tracing::info!("Cancel requested for task {id}");
        }
    }
    Ok(())
}
