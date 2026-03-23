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
use crate::events::{EventSender, UiEvent};
use crate::permission::{self, Action, PendingMap, Rule};
use crate::prompt;
use crate::snapshot::Snapshots;
use crate::store::Store;
use crate::tools;

/// Maximum output tokens per task before forced wrap-up.
const MAX_TASK_OUTPUT_TOKENS: u32 = 500_000;
/// Warning threshold (80% of max).
const TOKEN_BUDGET_WARNING: u32 = MAX_TASK_OUTPUT_TOKENS * 80 / 100;

/// A tool call being accumulated from the stream.
struct PendingToolCall {
    id: String,
    name: String,
    args_json: String,
}

/// Run a single agent task to completion, sending events to the TUI.
pub async fn run_task(
    provider: &dyn Provider,
    model: &str,
    task_id: String,
    prompt: String,
    image: Option<(String, String)>,
    plan_mode: bool,
    cancel: CancellationToken,
    store: &Store,
    pending_permissions: &PendingMap,
    ui: &EventSender,
) -> Result<()> {
    info!("starting task {task_id}");

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if let Err(e) = store.create_session(&task_id, &cwd).await {
        tracing::warn!("failed to create session: {e}");
    }

    // Initialize snapshot system for undo support
    let snapshots = std::env::current_dir()
        .ok()
        .and_then(|cwd| Snapshots::new(&cwd).ok());
    let initial_snapshot = snapshots.as_ref().and_then(|s| s.track().ok());

    let mut system = prompt::build_system_prompt(model);
    let tool_defs = if plan_mode {
        system.push_str(
            "\n\n# Mode: PLAN\n\
             You are in read-only planning mode. You MUST NOT modify any files.\n\
             Explore the codebase, understand the problem, and produce a structured plan.\n\
             Use this format:\n\
             ## Plan\n\
             1. [file] — description of change\n\
             2. [file] — description of change\n\
             ...\n\
             Do NOT write code. Describe what to change and why."
        );
        tools::read_only_definitions()
    } else {
        tools::definitions()
    };
    let default_rules = permission::default_rules();
    let mut session_rules: Vec<Rule> = Vec::new();

    let prompt_for_title = prompt.clone();
    let mut user_content: Vec<ContentBlock> = vec![ContentBlock::Text { text: prompt }];
    if let Some((media_type, data)) = image {
        user_content.push(ContentBlock::Image { media_type, data });
    }
    let mut messages = vec![Message::User {
        content: user_content,
    }];
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;
    let mut step: u32 = 0;
    let mut recent_tool_calls: Vec<(String, String)> = Vec::new(); // (tool_name, args_hash)

    loop {
        if cancel.is_cancelled() {
            send_cancelled(&task_id, ui).await;
            return Ok(());
        }

        let request = StreamRequest {
            model: model.to_string(),
            system: system.clone(),
            messages: messages.clone(),
            tools: tool_defs.clone(),
            max_tokens: crate::models::max_output(model),
        };

        let (stop_reason, usage) = stream_response(
            provider, request, &task_id, &cancel, &mut messages, ui,
        )
        .await?;

        step += 1;

        // Accumulate token usage
        if let Some(ref u) = usage {
            total_input_tokens += u.input_tokens;
            total_output_tokens += u.output_tokens;
        }

        // Set title on first step from the prompt
        if step == 1 {
            let title = generate_title_from_prompt(&prompt_for_title);
            let _ = store.update_title(&task_id, &title).await;
        }

        // Token budget enforcement
        if total_output_tokens > MAX_TASK_OUTPUT_TOKENS {
            info!("task {task_id}: token budget exhausted ({total_output_tokens} output tokens)");
            ui.send(UiEvent::Status {
                id: task_id.clone(),
                message: "Token budget exhausted. Wrapping up.".into(),
            })
            .await;
            break;
        } else if total_output_tokens > TOKEN_BUDGET_WARNING && step > 1 {
            info!("task {task_id}: approaching token budget ({total_output_tokens}/{MAX_TASK_OUTPUT_TOKENS})");
        }

        // Check if compaction is needed
        if let Some(ref u) = usage {
            if compaction::should_compact(u.input_tokens, model) {
                info!("task {task_id}: approaching context limit, pruning...");
                let freed = compaction::prune_tool_outputs(&mut messages);
                info!("task {task_id}: pruned {freed} bytes");

                if compaction::should_compact(
                    u.input_tokens.saturating_sub(freed as u32 / 4),
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
                let tool_calls = extract_tool_calls(&messages);
                if tool_calls.is_empty() {
                    info!("task {task_id}: tool_use stop reason but no tool calls found");
                    break;
                }

                // Loop detection: check for repeated identical tool calls
                let mut doom_loop = false;
                for (_call_id, tool_name, input) in &tool_calls {
                    let args_hash = format!("{}:{}", tool_name, input);
                    recent_tool_calls.push((tool_name.clone(), args_hash));
                }
                if recent_tool_calls.len() >= 3 {
                    let last = &recent_tool_calls[recent_tool_calls.len() - 1];
                    let prev1 = &recent_tool_calls[recent_tool_calls.len() - 2];
                    let prev2 = &recent_tool_calls[recent_tool_calls.len() - 3];
                    if last.1 == prev1.1 && prev1.1 == prev2.1 {
                        doom_loop = true;
                        info!(
                            "task {task_id}: doom loop detected — {} called 3x with same args",
                            last.0
                        );
                    }
                }
                if doom_loop {
                    ui.send(UiEvent::Error {
                        id: task_id.clone(),
                        message: "Loop detected: same tool called 3x with identical args. Stopping to avoid wasting tokens.".into(),
                    }).await;
                    break;
                }

                let mut results = Vec::new();
                for (call_id, tool_name, input) in &tool_calls {
                    let perm = permission::tool_permission(tool_name);
                    let resource = permission::tool_resource(tool_name, input);
                    let action = permission::evaluate(
                        perm, &resource, &[&default_rules, &session_rules],
                    );

                    let (output, is_error) = match action {
                        Action::Deny => {
                            (format!("Permission denied: {perm} {resource}"), true)
                        }
                        Action::Allow | Action::Ask => {
                            if action == Action::Ask {
                                let req_id = format!("perm-{}-{}", task_id, call_id);
                                let (tx, rx) = tokio::sync::oneshot::channel();
                                pending_permissions.lock().await.insert(req_id.clone(), tx);

                                ui.send(UiEvent::PermissionRequest {
                                    id: task_id.clone(),
                                    request_id: req_id,
                                    tool: tool_name.clone(),
                                    resource: resource.clone(),
                                })
                                .await;

                                match rx.await {
                                    Ok(permission::Decision::Once) => {}
                                    Ok(permission::Decision::Always) => {
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

                            ui.send(UiEvent::ToolCall {
                                id: task_id.clone(),
                                tool: tool_name.clone(),
                                input: input.clone(),
                            })
                            .await;

                            match tools::execute(tool_name, input.clone(), &cancel).await {
                                Ok(output) => (output, false),
                                Err(e) => (format!("Error: {e:#}"), true),
                            }
                        }
                    };

                    ui.send(UiEvent::ToolResult {
                        id: task_id.clone(),
                        tool: tool_name.clone(),
                        output: output.clone(),
                        is_error,
                    })
                    .await;

                    results.push(ContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        content: output,
                        is_error,
                    });
                }

                // Compiler-in-the-loop: if any tools modified files, run a
                // build check and inject diagnostics so the agent can fix
                // errors in the same turn.
                let tool_names: Vec<&str> = tool_calls.iter().map(|(_, n, _)| n.as_str()).collect();
                if crate::lsp::modifies_files(&tool_names) {
                    let (build_ok, diags) = crate::lsp::check_project();
                    if !build_ok && !diags.is_empty() {
                        let diag_text = format!(
                            "Build check FAILED after your edits. Fix these errors:\n{}",
                            diags.join("\n")
                        );
                        info!("task {task_id}: build failed, injecting {} diagnostics", diags.len());
                        results.push(ContentBlock::ToolResult {
                            tool_use_id: "build-check".into(),
                            content: diag_text,
                            is_error: true,
                        });
                        ui.send(UiEvent::ToolResult {
                            id: task_id.clone(),
                            tool: "build_check".into(),
                            output: format!("{} errors found", diags.len()),
                            is_error: true,
                        })
                        .await;
                    } else if !diags.is_empty() {
                        // Warnings only
                        let warn_text = format!(
                            "Build passed with warnings:\n{}",
                            diags.join("\n")
                        );
                        results.push(ContentBlock::ToolResult {
                            tool_use_id: "build-check".into(),
                            content: warn_text,
                            is_error: false,
                        });
                    }
                }

                messages.push(Message::User { content: results });
                info!("task {task_id}: executed {} tools, re-prompting", tool_calls.len());
            }
            _ => break,
        }
    }

    if let Err(e) = store.save_messages(&task_id, &messages).await {
        tracing::warn!("failed to save messages: {e}");
    }

    ui.send(UiEvent::Done {
        id: task_id.clone(),
        input_tokens: total_input_tokens,
        output_tokens: total_output_tokens,
    })
    .await;
    info!("task {task_id} done");
    Ok(())
}

/// Stream a single LLM response, accumulating text and tool calls.
async fn stream_response(
    provider: &dyn Provider,
    request: StreamRequest,
    task_id: &str,
    cancel: &CancellationToken,
    messages: &mut Vec<Message>,
    ui: &EventSender,
) -> Result<(Option<StopReason>, Option<Usage>)> {
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
                    ui.send(UiEvent::Status {
                        id: task_id.to_string(),
                        message: format!("{}, retrying in {secs:.0}s...", ce.message),
                    })
                    .await;
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => continue,
                        _ = cancel.cancelled() => {
                            send_cancelled(task_id, ui).await;
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
                send_cancelled(task_id, ui).await;
                return Ok((None, None));
            }
            event = rx.recv() => {
                match event {
                    Some(Ok(StreamEvent::TextStart { index })) => {
                        text_parts.entry(index).or_default();
                    }
                    Some(Ok(StreamEvent::TextDelta { index, text })) => {
                        ui.send(UiEvent::Token {
                            id: task_id.to_string(),
                            content: text.clone(),
                        }).await;
                        text_parts.entry(index).or_default().push_str(&text);
                    }
                    Some(Ok(StreamEvent::ToolStart { index, id, name })) => {
                        tool_calls.insert(index, PendingToolCall {
                            id, name, args_json: String::new(),
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
                    Some(Ok(StreamEvent::MessageStop)) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        ui.send(UiEvent::Error {
                            id: task_id.to_string(),
                            message: format!("stream error: {e}"),
                        }).await;
                        return Ok((None, None));
                    }
                }
            }
        }
    }

    // Build assistant message from accumulated parts
    let mut content: Vec<ContentBlock> = Vec::new();

    let mut text_indices: Vec<u32> = text_parts.keys().copied().collect();
    text_indices.sort();
    for idx in text_indices {
        if let Some(text) = text_parts.remove(&idx) {
            if !text.is_empty() {
                content.push(ContentBlock::Text { text });
            }
        }
    }

    let mut tool_indices: Vec<u32> = tool_calls.keys().copied().collect();
    tool_indices.sort();
    for idx in tool_indices {
        if let Some(tc) = tool_calls.remove(&idx) {
            let input: serde_json::Value = serde_json::from_str(&tc.args_json)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
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

/// Generate a short title from the user's prompt.
/// Takes first line, removes articles, caps at 50 chars.
fn generate_title_from_prompt(prompt: &str) -> String {
    let first_line = prompt.lines().next().unwrap_or(prompt);
    let cleaned: String = first_line
        .split_whitespace()
        .filter(|w| !matches!(w.to_lowercase().as_str(), "the" | "a" | "an" | "this" | "my" | "please" | "can" | "you"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut title: String = cleaned.chars().take(50).collect();
    if cleaned.len() > 50 {
        title.push_str("...");
    }
    if title.is_empty() {
        title = "untitled".to_string();
    }
    title
}

async fn send_cancelled(task_id: &str, ui: &EventSender) {
    info!("task {task_id} cancelled");
    ui.send(UiEvent::Error {
        id: task_id.to_string(),
        message: "cancelled".into(),
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_removes_articles() {
        assert_eq!(
            generate_title_from_prompt("Fix the bug in the auth module"),
            "Fix bug in auth module"
        );
    }

    #[test]
    fn test_title_caps_at_50() {
        let long = "Implement a comprehensive refactoring of the entire authentication and authorization subsystem";
        let title = generate_title_from_prompt(long);
        assert!(title.len() <= 53); // 50 + "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_title_first_line_only() {
        assert_eq!(
            generate_title_from_prompt("Fix tests\nAlso refactor the module"),
            "Fix tests"
        );
    }

    #[test]
    fn test_title_empty_prompt() {
        assert_eq!(generate_title_from_prompt(""), "untitled");
    }

    #[test]
    fn test_title_all_articles() {
        assert_eq!(generate_title_from_prompt("the a an"), "untitled");
    }

    #[test]
    fn test_doom_loop_detection() {
        let mut recent: Vec<(String, String)> = Vec::new();
        let call = ("read_file".to_string(), "read_file:{\"path\":\"foo.rs\"}".to_string());
        recent.push(call.clone());
        recent.push(call.clone());
        recent.push(call.clone());

        let last = &recent[recent.len() - 1];
        let prev1 = &recent[recent.len() - 2];
        let prev2 = &recent[recent.len() - 3];
        assert_eq!(last.1, prev1.1);
        assert_eq!(prev1.1, prev2.1);
    }

    #[test]
    fn test_no_doom_loop_different_args() {
        let recent = vec![
            ("read_file".to_string(), "read_file:{\"path\":\"a.rs\"}".to_string()),
            ("read_file".to_string(), "read_file:{\"path\":\"b.rs\"}".to_string()),
            ("read_file".to_string(), "read_file:{\"path\":\"c.rs\"}".to_string()),
        ];
        let last = &recent[recent.len() - 1];
        let prev1 = &recent[recent.len() - 2];
        assert_ne!(last.1, prev1.1);
    }

    #[test]
    fn test_token_budget_constants() {
        assert!(TOKEN_BUDGET_WARNING < MAX_TASK_OUTPUT_TOKENS);
        assert_eq!(TOKEN_BUDGET_WARNING, MAX_TASK_OUTPUT_TOKENS * 80 / 100);
    }
}
