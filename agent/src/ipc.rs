//! Newline-delimited JSON IPC over stdin/stdout.
//!
//! Message format: one JSON object per line.
//! Incoming (from Emacs): Request
//! Outgoing (to Emacs): Response

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::api::Provider;

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
    ToolCall {
        id: String,
        tool: String,
        input: serde_json::Value,
    },
    /// A tool has completed.
    ToolResult {
        id: String,
        tool: String,
        output: String,
    },
    /// Task completed successfully.
    Done { id: String },
    /// An error occurred.
    Error { id: String, message: String },
    /// Status update (retry, busy, idle).
    Status { id: String, message: String },
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

/// Tracks in-flight tasks so they can be cancelled.
type TaskMap = Arc<Mutex<HashMap<String, CancellationToken>>>;

/// Main IPC loop: read requests from stdin, dispatch to agent.
pub async fn run_loop(provider: Arc<dyn Provider>, model: String) -> Result<()> {
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();
    let tasks: TaskMap = Arc::new(Mutex::new(HashMap::new()));

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                handle_request(request, &provider, &model, &tasks).await?;
            }
            Err(e) => {
                tracing::error!("Failed to parse request: {e}: {line}");
            }
        }
    }

    Ok(())
}

async fn handle_request(
    request: Request,
    provider: &Arc<dyn Provider>,
    model: &str,
    tasks: &TaskMap,
) -> Result<()> {
    match request {
        Request::Ping => {
            send(&Response::Pong).await?;
        }
        Request::Task { id, prompt } => {
            let cancel = CancellationToken::new();
            tasks.lock().await.insert(id.clone(), cancel.clone());

            let provider = Arc::clone(provider);
            let model = model.to_string();
            let tasks = Arc::clone(tasks);
            let task_id = id.clone();

            tokio::spawn(async move {
                let result =
                    crate::agent::run_task(&*provider, &model, task_id.clone(), prompt, cancel)
                        .await;
                if let Err(e) = &result {
                    let _ = send(&Response::Error {
                        id: task_id.clone(),
                        message: format!("{e:#}"),
                    })
                    .await;
                }
                tasks.lock().await.remove(&task_id);
            });
        }
        Request::Cancel { id } => {
            if let Some(cancel) = tasks.lock().await.get(&id) {
                tracing::info!("Cancelling task {id}");
                cancel.cancel();
            } else {
                tracing::info!("Cancel requested for unknown task {id}");
            }
        }
    }
    Ok(())
}
