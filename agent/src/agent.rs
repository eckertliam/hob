//! Agent loop: orchestrates API calls and tool dispatch.
//!
//! Runs a while(true) loop: call the LLM, if it wants tools execute them
//! and re-prompt with results, if it says stop then break.

use std::collections::HashMap;

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::api::{ContentBlock, Message, Provider, StopReason, StreamEvent, StreamRequest, Usage};
use crate::compaction;
use crate::error::{self, ClassifiedError};
use crate::ipc;
use crate::permission::{self, Action, Decision, PendingMap, Rule};
use crate::prompt;
use crate::store::Store;
use crate::tools;

/// A tool call being accumulated from the stream.
struct PendingToolCall {
    id: String,
    name: String,
    args_json: String,
}

/// Run a single agent task to completion, sending IPC responses along the way.
///
/// The loop:
/// 1. Call the LLM with the current message history + tool definitions
/// 2. Stream tokens back to Emacs
/// 3. Accumulate any tool calls from the stream
/// 4. If stop_reason == ToolUse: execute tools, append results, go to 1
/// 5. If stop_reason == EndTurn (or other): break
pub async fn run_task(
    provider: &dyn Provider,
    model: &str,
    task_id: String,
    prompt: String,
    cancel: CancellationToken,
    store: &Store,
    pending_permissions: &PendingMap,
) -> Result<()> {
    info!("starting task {task_id}");

    // Create session for persistence
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if let Err(e) = store.create_session(&task_id, &cwd).await {
        tracing::warn!("failed to create session: {e}");
    }

    let system = prompt::build_system_prompt(model);
    let tool_defs = tools::definitions();
    let default_rules = permission::default_rules();
    let mut session_rules: Vec<Rule> = Vec::new();

    let mut messages = vec![Message::User {
        content: vec![ContentBlock::Text { text: prompt }],
    }];

    loop {
        if cancel.is_cancelled() {
            send_cancelled(&task_id).await?;
            return Ok(());
        }

        let request = StreamRequest {
            model: model.to_string(),
            system: system.clone(),
            messages: messages.clone(),
            tools: tool_defs.clone(),
            max_tokens: 16384,
        };

        let (stop_reason, usage) = stream_response(
            provider,
            request,
            &task_id,
            &cancel,
            &mut messages,
        )
        .await?;

        // Check if compaction is needed
        if let Some(ref u) = usage {
            if compaction::should_compact(u.input_tokens, model) {
                info!("task {task_id}: approaching context limit, pruning...");
                let freed = compaction::prune_tool_outputs(&mut messages);
                info!("task {task_id}: pruned {freed} bytes");

                // If pruning wasn't enough, summarize
                if compaction::should_compact(
                    u.input_tokens.saturating_sub(freed as u32 / 4), // rough estimate
                    model,
                ) {
                    info!("task {task_id}: pruning insufficient, summarizing...");
                    match compaction::summarize(provider, model, &messages).await {
                        Ok(summary) => {
                            compaction::compact(&mut messages, summary, true);
                            info!("task {task_id}: compacted to {} messages", messages.len());
                        }
                        Err(e) => {
                            tracing::warn!("compaction failed: {e}");
                        }
                    }
                }
            }
        }

        match stop_reason {
            Some(StopReason::ToolUse) => {
                // Execute tool calls that were accumulated and appended to messages
                // by stream_response. The last message should be an assistant message
                // with tool_use blocks. We need to execute them and append results.
                let tool_calls = extract_tool_calls(&messages);
                if tool_calls.is_empty() {
                    info!("task {task_id}: tool_use stop reason but no tool calls found");
                    break;
                }

                let mut results = Vec::new();
                for (call_id, tool_name, input) in &tool_calls {
                    // Check permissions
                    let perm = permission::tool_permission(tool_name);
                    let resource = permission::tool_resource(tool_name, input);
                    let action = permission::evaluate(
                        perm,
                        &resource,
                        &[&default_rules, &session_rules],
                    );

                    let (output, is_error) = match action {
                        Action::Deny => {
                            (format!("Permission denied: {perm} {resource}"), true)
                        }
                        Action::Allow | Action::Ask => {
                            // If Ask, request permission from Emacs
                            if action == Action::Ask {
                                let req_id = format!("perm-{}-{}", task_id, call_id);
                                let (tx, rx) = tokio::sync::oneshot::channel();
                                pending_permissions.lock().await.insert(req_id.clone(), tx);

                                ipc::send(&ipc::Response::PermissionRequest {
                                    id: task_id.clone(),
                                    request_id: req_id,
                                    tool: tool_name.clone(),
                                    resource: resource.clone(),
                                })
                                .await?;

                                // Wait for user decision
                                match rx.await {
                                    Ok(Decision::Once) => {}
                                    Ok(Decision::Always) => {
                                        session_rules.push(Rule {
                                            permission: perm.to_string(),
                                            pattern: "*".into(),
                                            action: Action::Allow,
                                        });
                                    }
                                    _ => {
                                        results.push(ContentBlock::ToolResult {
                                            tool_use_id: call_id.clone(),
                                            content: "Permission denied by user".into(),
                                            is_error: true,
                                        });
                                        continue;
                                    }
                                }
                            }

                            // Notify Emacs and execute
                            ipc::send(&ipc::Response::ToolCall {
                                id: task_id.clone(),
                                tool: tool_name.clone(),
                                input: input.clone(),
                            })
                            .await?;

                            match tools::execute(tool_name, input.clone(), &cancel).await {
                                Ok(output) => (output, false),
                                Err(e) => (format!("Error: {e:#}"), true),
                            }
                        }
                    };

                    ipc::send(&ipc::Response::ToolResult {
                        id: task_id.clone(),
                        tool: tool_name.clone(),
                        output: output.clone(),
                    })
                    .await?;

                    results.push(ContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        content: output,
                        is_error,
                    });
                }

                // Append tool results as a user message and loop back
                messages.push(Message::User { content: results });
                info!("task {task_id}: executed {} tools, re-prompting", tool_calls.len());
            }
            _ => {
                // EndTurn, MaxTokens, or anything else — we're done
                break;
            }
        }
    }

    // Persist messages before signaling completion
    if let Err(e) = store.save_messages(&task_id, &messages).await {
        tracing::warn!("failed to save messages: {e}");
    }

    ipc::send(&ipc::Response::Done {
        id: task_id.clone(),
    })
    .await?;

    info!("task {task_id} done");
    Ok(())
}

/// Stream a single LLM response, accumulating text and tool calls.
///
/// Appends the assistant message (with text + tool_use blocks) to `messages`.
/// Returns the stop reason.
async fn stream_response(
    provider: &dyn Provider,
    request: StreamRequest,
    task_id: &str,
    cancel: &CancellationToken,
    messages: &mut Vec<Message>,
) -> Result<(Option<StopReason>, Option<Usage>)> {
    // Retry loop for transient API errors
    let mut attempt = 0u32;
    let mut rx = loop {
        match provider.stream(request.clone()).await {
            Ok(rx) => break rx,
            Err(e) => {
                if let Some(ce) = e.downcast_ref::<ClassifiedError>() {
                    if !error::is_retryable(&ce.kind) {
                        return Err(e);
                    }
                    attempt += 1;
                    let delay = error::retry_delay(attempt, ce.retry_after);
                    let secs = delay.as_secs_f64();
                    info!("task {task_id}: {}, retrying in {secs:.1}s (attempt {attempt})", ce.message);
                    ipc::send(&ipc::Response::Status {
                        id: task_id.to_string(),
                        message: format!("{}, retrying in {secs:.0}s...", ce.message),
                    })
                    .await?;
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => continue,
                        _ = cancel.cancelled() => {
                            send_cancelled(task_id).await?;
                            return Ok((None, None));
                        }
                    }
                } else {
                    return Err(e);
                }
            }
        }
    };

    let mut text_parts: HashMap<u32, String> = HashMap::new();
    let mut tool_calls: HashMap<u32, PendingToolCall> = HashMap::new();
    let mut stop_reason: Option<StopReason> = None;
    let mut last_usage: Option<Usage> = None;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                send_cancelled(task_id).await?;
                return Ok((None, None));
            }
            event = rx.recv() => {
                match event {
                    Some(Ok(StreamEvent::TextStart { index })) => {
                        text_parts.entry(index).or_default();
                    }
                    Some(Ok(StreamEvent::TextDelta { index, text })) => {
                        ipc::send(&ipc::Response::Token {
                            id: task_id.to_string(),
                            content: text.clone(),
                        })
                        .await?;
                        text_parts.entry(index).or_default().push_str(&text);
                    }
                    Some(Ok(StreamEvent::ToolStart { index, id, name })) => {
                        tool_calls.insert(index, PendingToolCall {
                            id,
                            name,
                            args_json: String::new(),
                        });
                    }
                    Some(Ok(StreamEvent::ToolDelta { index, args_json })) => {
                        if let Some(tc) = tool_calls.get_mut(&index) {
                            tc.args_json.push_str(&args_json);
                        }
                    }
                    Some(Ok(StreamEvent::MessageDelta { stop_reason: sr, usage })) => {
                        stop_reason = sr;
                        if usage.is_some() {
                            last_usage = usage;
                        }
                    }
                    Some(Ok(StreamEvent::MessageStop)) | None => {
                        break;
                    }
                    Some(Ok(_)) => {
                        // TextStop, ToolStop — no action needed
                    }
                    Some(Err(e)) => {
                        ipc::send(&ipc::Response::Error {
                            id: task_id.to_string(),
                            message: format!("stream error: {e}"),
                        })
                        .await?;
                        return Ok((None, None));
                    }
                }
            }
        }
    }

    // Build the assistant message from accumulated parts
    let mut content: Vec<ContentBlock> = Vec::new();

    // Add text blocks (sorted by index)
    let mut text_indices: Vec<u32> = text_parts.keys().copied().collect();
    text_indices.sort();
    for idx in text_indices {
        if let Some(text) = text_parts.remove(&idx) {
            if !text.is_empty() {
                content.push(ContentBlock::Text { text });
            }
        }
    }

    // Add tool call blocks (sorted by index)
    let mut tool_indices: Vec<u32> = tool_calls.keys().copied().collect();
    tool_indices.sort();
    for idx in tool_indices {
        if let Some(tc) = tool_calls.remove(&idx) {
            let input: serde_json::Value =
                serde_json::from_str(&tc.args_json).unwrap_or(serde_json::Value::Object(
                    serde_json::Map::new(),
                ));
            content.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.name,
                input,
            });
        }
    }

    if !content.is_empty() {
        messages.push(Message::Assistant { content });
    }

    Ok((stop_reason, last_usage))
}

/// Extract tool calls from the last assistant message.
fn extract_tool_calls(messages: &[Message]) -> Vec<(String, String, serde_json::Value)> {
    let Some(Message::Assistant { content }) = messages.last() else {
        return vec![];
    };
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => {
                Some((id.clone(), name.clone(), input.clone()))
            }
            _ => None,
        })
        .collect()
}

async fn send_cancelled(task_id: &str) -> Result<()> {
    info!("task {task_id} cancelled");
    ipc::send(&ipc::Response::Error {
        id: task_id.to_string(),
        message: "cancelled".into(),
    })
    .await
}
