//! Context window management: prune old tool outputs and summarize.
//!
//! Two-phase strategy:
//! 1. Prune: clear old tool outputs (cheap)
//! 2. Summarize: LLM call to compress history (expensive)

use anyhow::Result;
use tracing::info;

use crate::api::{
    ContentBlock, Message, Provider, StreamEvent, StreamRequest,
};

/// Minimum bytes to prune before it's worth doing.
const PRUNE_MINIMUM: usize = 20_000;
/// Target utilization ratio — compact at 50% to maintain quality.
/// Research shows output degrades beyond ~40% context utilization.
const TARGET_UTILIZATION: f64 = 0.5;
/// Protect the last N user turns from pruning.
const PRUNE_PROTECT_TURNS: usize = 2;
/// Buffer tokens before triggering compaction.
const COMPACTION_BUFFER: u32 = 20_000;

const SUMMARIZE_PROMPT: &str = "\
Provide a detailed summary for continuing our conversation. Use this template:

## Goal
What is the user trying to accomplish?

## Key Decisions
What important decisions have been made?

## What Was Accomplished
What work has been completed so far?

## Current State
Where did we leave off? What's next?

## Relevant Files
Which files were read or modified?";

/// Check if we should trigger compaction based on token usage.
pub fn should_compact(input_tokens: u32, model: &str) -> bool {
    let limit = crate::models::context_limit(model);
    let target = (limit as f64 * TARGET_UTILIZATION) as u32;
    input_tokens > target
}

/// Phase 1: Prune old tool outputs.
///
/// Walks backward through messages, replacing old ToolResult content with
/// a placeholder. Preserves the last `PRUNE_PROTECT_TURNS` user turns.
/// Returns the number of bytes freed.
pub fn prune_tool_outputs(messages: &mut Vec<Message>) -> usize {
    let mut bytes_freed = 0;

    // Count user turns from the end to find the protection boundary
    let mut user_turns_seen = 0;
    let mut protect_boundary = messages.len();
    for (i, msg) in messages.iter().enumerate().rev() {
        if matches!(msg, Message::User { .. }) {
            user_turns_seen += 1;
            if user_turns_seen >= PRUNE_PROTECT_TURNS {
                protect_boundary = i;
                break;
            }
        }
    }

    // Prune tool outputs before the boundary
    for msg in messages[..protect_boundary].iter_mut() {
        if let Message::User { content } = msg {
            for block in content.iter_mut() {
                if let ContentBlock::ToolResult {
                    content, is_error, ..
                } = block
                {
                    if !*is_error && content != "[Old tool result cleared]" {
                        bytes_freed += content.len();
                        *content = "[Old tool result cleared]".to_string();
                    }
                }
            }
        }
    }

    if bytes_freed < PRUNE_MINIMUM {
        // Not worth it, but we already mutated. That's fine — the cleared
        // content is not needed anyway.
    }

    bytes_freed
}

/// Phase 2: Summarize the conversation via an LLM call.
///
/// Returns the summary text.
pub async fn summarize(
    provider: &dyn Provider,
    model: &str,
    messages: &[Message],
) -> Result<String> {
    let mut summary_messages: Vec<Message> = messages.to_vec();
    summary_messages.push(Message::User {
        content: vec![ContentBlock::Text {
            text: SUMMARIZE_PROMPT.to_string(),
        }],
    });

    let request = StreamRequest {
        model: model.to_string(),
        system: "You are a conversation summarizer. Produce a concise but thorough summary."
            .to_string(),
        messages: summary_messages,
        tools: vec![],
        max_tokens: 4096,
    };

    let mut rx = provider.stream(request).await?;
    let mut text = String::new();

    while let Some(event) = rx.recv().await {
        match event {
            Ok(StreamEvent::TextDelta { text: delta, .. }) => {
                text.push_str(&delta);
            }
            Ok(StreamEvent::MessageStop) | Err(_) => break,
            _ => {}
        }
    }

    info!("compaction summary: {} chars", text.len());
    Ok(text)
}

/// Replace messages with a summary, optionally replaying the last user request.
pub fn compact(messages: &mut Vec<Message>, summary: String, replay_last: bool) {
    // Save the last user message for replay
    let last_user = if replay_last {
        messages
            .iter()
            .rev()
            .find(|m| matches!(m, Message::User { .. }))
            .cloned()
    } else {
        None
    };

    messages.clear();

    // Insert summary as a user→assistant exchange
    messages.push(Message::User {
        content: vec![ContentBlock::Text {
            text: "What did we do so far?".to_string(),
        }],
    });
    messages.push(Message::Assistant {
        content: vec![ContentBlock::Text { text: summary }],
    });

    // Replay the last user request so the model continues
    if let Some(user_msg) = last_user {
        messages.push(user_msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages_with_tool_results() -> Vec<Message> {
        vec![
            Message::User {
                content: vec![ContentBlock::Text {
                    text: "do something".into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({}),
                }],
            },
            Message::User {
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "x".repeat(10_000),
                    is_error: false,
                }],
            },
            // Recent turns (protected)
            Message::User {
                content: vec![ContentBlock::Text {
                    text: "now do this".into(),
                }],
            },
            Message::Assistant {
                content: vec![ContentBlock::Text {
                    text: "ok".into(),
                }],
            },
            Message::User {
                content: vec![ContentBlock::Text {
                    text: "and this".into(),
                }],
            },
        ]
    }

    #[test]
    fn test_prune_replaces_old_tool_outputs() {
        let mut msgs = make_messages_with_tool_results();
        let freed = prune_tool_outputs(&mut msgs);
        assert!(freed > 0);

        // The old tool result should be cleared
        if let Message::User { content } = &msgs[2] {
            if let ContentBlock::ToolResult { content, .. } = &content[0] {
                assert_eq!(content, "[Old tool result cleared]");
            } else {
                panic!("expected ToolResult");
            }
        }
    }

    #[test]
    fn test_prune_preserves_recent_turns() {
        let mut msgs = make_messages_with_tool_results();
        prune_tool_outputs(&mut msgs);

        // Recent user messages should still have their original content
        if let Message::User { content } = &msgs[3] {
            if let ContentBlock::Text { text } = &content[0] {
                assert_eq!(text, "now do this");
            }
        }
    }

    #[test]
    fn test_prune_returns_bytes_freed() {
        let mut msgs = make_messages_with_tool_results();
        let freed = prune_tool_outputs(&mut msgs);
        assert_eq!(freed, 10_000);
    }

    #[test]
    fn test_compact_replaces_messages_with_summary() {
        let mut msgs = make_messages_with_tool_results();
        compact(&mut msgs, "Summary of work done.".into(), false);
        assert_eq!(msgs.len(), 2); // user question + assistant summary
    }

    #[test]
    fn test_compact_with_replay() {
        let mut msgs = make_messages_with_tool_results();
        compact(&mut msgs, "Summary.".into(), true);
        assert_eq!(msgs.len(), 3); // question + summary + replayed user msg
    }

    #[test]
    fn test_should_compact() {
        // claude-sonnet-4-6 has 1M context, 50% target = 500K
        assert!(!should_compact(400_000, "claude-sonnet-4-6"));
        assert!(should_compact(600_000, "claude-sonnet-4-6"));
        // Unknown model defaults to 200K, 50% = 100K
        assert!(!should_compact(80_000, "unknown-model"));
        assert!(should_compact(120_000, "unknown-model"));
    }
}
