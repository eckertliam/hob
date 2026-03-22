//! Agent loop: orchestrates API calls and tool dispatch.

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::api::{ContentBlock, Message, Provider, StreamEvent, StreamRequest};
use crate::ipc;

/// Build the system prompt with environment context.
fn build_system_prompt() -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let platform = std::env::consts::OS;

    format!(
        "You are a helpful AI coding assistant.\n\n\
         # Environment\n\
         - Working directory: {cwd}\n\
         - Platform: {platform}\n"
    )
}

/// Run a single agent task to completion, sending IPC responses along the way.
pub async fn run_task(
    provider: &dyn Provider,
    model: &str,
    task_id: String,
    prompt: String,
    cancel: CancellationToken,
) -> Result<()> {
    info!("starting task {task_id}");

    let messages = vec![Message::User {
        content: vec![ContentBlock::Text { text: prompt }],
    }];

    let request = StreamRequest {
        model: model.to_string(),
        system: build_system_prompt(),
        messages,
        tools: vec![],
        max_tokens: 16384,
    };

    let mut rx = provider.stream(request).await?;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("task {task_id} cancelled");
                ipc::send(&ipc::Response::Error {
                    id: task_id.clone(),
                    message: "cancelled".into(),
                })
                .await?;
                return Ok(());
            }
            event = rx.recv() => {
                match event {
                    Some(Ok(StreamEvent::TextDelta { text, .. })) => {
                        ipc::send(&ipc::Response::Token {
                            id: task_id.clone(),
                            content: text,
                        })
                        .await?;
                    }
                    Some(Ok(StreamEvent::MessageDelta { stop_reason, .. })) => {
                        if let Some(reason) = stop_reason {
                            info!("task {task_id} stop_reason: {reason:?}");
                        }
                    }
                    Some(Ok(StreamEvent::MessageStop)) | None => {
                        break;
                    }
                    Some(Ok(_)) => {
                        // TextStart, TextStop, Tool* events — ignored for single-turn
                    }
                    Some(Err(e)) => {
                        ipc::send(&ipc::Response::Error {
                            id: task_id.clone(),
                            message: format!("stream error: {e}"),
                        })
                        .await?;
                        return Ok(());
                    }
                }
            }
        }
    }

    ipc::send(&ipc::Response::Done {
        id: task_id.clone(),
    })
    .await?;

    info!("task {task_id} done");
    Ok(())
}
