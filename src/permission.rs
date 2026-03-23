//! Permission system: last-match-wins wildcard rules.
//!
//! Gates tool execution behind allow/deny/ask decisions.
//! When "ask" is needed, sends an IPC request to Emacs and blocks
//! until the user responds.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, oneshot};

/// What to do with a permission request.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Allow,
    Deny,
    Ask,
}

/// A single permission rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pub permission: String,
    pub pattern: String,
    pub action: Action,
}

/// The user's decision when asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Once,
    Always,
    Reject,
}

/// Map tool names to permission categories.
pub fn tool_permission(tool_name: &str) -> &'static str {
    match tool_name {
        "read_file" | "list_files" | "glob" | "grep" => "read",
        "write_file" | "edit_file" => "edit",
        "shell" => "bash",
        "web_fetch" => "webfetch",
        "web_search" => "websearch",
        _ => "unknown",
    }
}

/// Map tool name + input to the resource pattern to check.
pub fn tool_resource(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "read_file" | "write_file" | "edit_file" | "list_files" => input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string(),
        "shell" => input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string(),
        "glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string(),
        "grep" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string(),
        "web_fetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string(),
        "web_search" => input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string(),
        _ => "*".to_string(),
    }
}

/// Default permission rules.
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            permission: "read".into(),
            pattern: "*".into(),
            action: Action::Allow,
        },
        Rule {
            permission: "bash".into(),
            pattern: "*".into(),
            action: Action::Ask,
        },
        Rule {
            permission: "edit".into(),
            pattern: "*".into(),
            action: Action::Ask,
        },
        Rule {
            permission: "webfetch".into(),
            pattern: "*".into(),
            action: Action::Ask,
        },
        Rule {
            permission: "websearch".into(),
            pattern: "*".into(),
            action: Action::Allow,
        },
        Rule {
            permission: "unknown".into(),
            pattern: "*".into(),
            action: Action::Ask,
        },
    ]
}

/// Evaluate permission rules. Last matching rule wins.
pub fn evaluate(permission: &str, resource: &str, rulesets: &[&[Rule]]) -> Action {
    let mut result = Action::Ask;
    for ruleset in rulesets {
        for rule in *ruleset {
            if wildcard_match(permission, &rule.permission)
                && wildcard_match(resource, &rule.pattern)
            {
                result = rule.action.clone();
            }
        }
    }
    result
}

/// Glob-style wildcard matching: * matches any string, ? matches one char.
fn wildcard_match(text: &str, pattern: &str) -> bool {
    let mut ti = text.chars().peekable();
    let mut pi = pattern.chars().peekable();

    wildcard_match_recursive(&mut text.chars().collect::<Vec<_>>(), &pattern.chars().collect::<Vec<_>>(), 0, 0)
}

fn wildcard_match_recursive(text: &[char], pattern: &[char], ti: usize, pi: usize) -> bool {
    if pi == pattern.len() {
        return ti == text.len();
    }

    match pattern[pi] {
        '*' => {
            // * matches zero or more characters
            for i in ti..=text.len() {
                if wildcard_match_recursive(text, pattern, i, pi + 1) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < text.len() {
                wildcard_match_recursive(text, pattern, ti + 1, pi + 1)
            } else {
                false
            }
        }
        c => {
            if ti < text.len() && text[ti] == c {
                wildcard_match_recursive(text, pattern, ti + 1, pi + 1)
            } else {
                false
            }
        }
    }
}

/// Pending permission requests waiting for user response.
pub type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<Decision>>>>;

/// Create a new empty pending permissions map.
pub fn new_pending_map() -> PendingMap {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Resolve a pending permission request with the user's decision.
pub async fn resolve(pending: &PendingMap, request_id: &str, decision: Decision) {
    let mut map = pending.lock().await;

    if decision == Decision::Reject {
        // Cascade: reject all pending requests
        for (_id, sender) in map.drain() {
            let _ = sender.send(Decision::Reject);
        }
    } else if let Some(sender) = map.remove(request_id) {
        let _ = sender.send(decision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_match_star() {
        assert!(wildcard_match("foo.rs", "*.rs"));
        assert!(wildcard_match("anything", "*"));
        assert!(!wildcard_match("foo.rs", "*.txt"));
    }

    #[test]
    fn test_wildcard_match_question() {
        assert!(wildcard_match("a.rs", "?.rs"));
        assert!(!wildcard_match("ab.rs", "?.rs"));
    }

    #[test]
    fn test_wildcard_match_exact() {
        assert!(wildcard_match("hello", "hello"));
        assert!(!wildcard_match("hello", "world"));
    }

    #[test]
    fn test_evaluate_last_match_wins() {
        let rules = vec![
            Rule {
                permission: "bash".into(),
                pattern: "*".into(),
                action: Action::Deny,
            },
            Rule {
                permission: "bash".into(),
                pattern: "*".into(),
                action: Action::Allow,
            },
        ];
        assert_eq!(evaluate("bash", "ls", &[&rules]), Action::Allow);
    }

    #[test]
    fn test_evaluate_default_ask() {
        let rules: Vec<Rule> = vec![];
        assert_eq!(evaluate("anything", "anything", &[&rules]), Action::Ask);
    }

    #[test]
    fn test_default_rules_allow_read() {
        let defaults = default_rules();
        assert_eq!(
            evaluate("read", "/any/path", &[&defaults]),
            Action::Allow
        );
    }

    #[test]
    fn test_default_rules_ask_bash() {
        let defaults = default_rules();
        assert_eq!(evaluate("bash", "rm -rf /", &[&defaults]), Action::Ask);
    }

    #[test]
    fn test_default_rules_ask_edit() {
        let defaults = default_rules();
        assert_eq!(evaluate("edit", "/any/path", &[&defaults]), Action::Ask);
    }

    #[test]
    fn test_tool_permission_mapping() {
        assert_eq!(tool_permission("read_file"), "read");
        assert_eq!(tool_permission("shell"), "bash");
        assert_eq!(tool_permission("write_file"), "edit");
        assert_eq!(tool_permission("glob"), "read");
    }

    #[tokio::test]
    async fn test_resolve_reject_cascades() {
        let pending = new_pending_map();
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();
        pending.lock().await.insert("r1".into(), tx1);
        pending.lock().await.insert("r2".into(), tx2);

        resolve(&pending, "r1", Decision::Reject).await;

        assert_eq!(rx1.await.unwrap(), Decision::Reject);
        assert_eq!(rx2.await.unwrap(), Decision::Reject);
        assert!(pending.lock().await.is_empty());
    }
}
