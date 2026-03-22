//! System prompt assembly.
//!
//! Builds the system prompt from layers: base prompt, environment context,
//! and project instruction files (.hob.md).

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Max total size of instruction file content (50KB).
const MAX_INSTRUCTION_BYTES: usize = 50 * 1024;

const BASE_PROMPT: &str = "\
You are a helpful AI coding assistant. You help users with software engineering \
tasks: writing code, debugging, refactoring, explaining code, and running commands.

# Guidelines
- Read files before modifying them. Never edit a file you haven't seen.
- Use tools to gather information rather than guessing.
- Keep changes minimal and focused. Don't refactor code you weren't asked to touch.
- When running shell commands, prefer specific commands over broad ones.
- If a task is ambiguous, use the information available to make a reasonable choice \
rather than asking clarifying questions.
- Show your work by reading relevant files and running tests after making changes.";

/// Build the full system prompt.
pub fn build_system_prompt(model: &str) -> String {
    let mut parts = vec![BASE_PROMPT.to_string()];

    parts.push(environment_context(model));

    if let Ok(cwd) = std::env::current_dir() {
        if let Some(instructions) = load_instruction_files(&cwd) {
            parts.push(instructions);
        }
    }

    parts.join("\n\n")
}

/// Environment context: cwd, platform, git status, date, shell.
fn environment_context(model: &str) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let platform = std::env::consts::OS;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".into());
    let is_git = Path::new(".git").is_dir();

    let mut ctx = format!(
        "# Environment\n\
         - Model: {model}\n\
         - Working directory: {cwd}\n\
         - Platform: {platform}\n\
         - Shell: {shell}\n\
         - Git repository: {is_git}"
    );

    // Detect git branch if in a repo
    if is_git {
        if let Ok(output) = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .output()
        {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() {
                ctx.push_str(&format!("\n- Git branch: {branch}"));
            }
        }
    }

    ctx
}

/// Search for .hob.md files from cwd upward and concatenate their contents.
fn load_instruction_files(start: &Path) -> Option<String> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dir = start.to_path_buf();

    loop {
        let candidate = dir.join(".hob.md");
        if candidate.is_file() {
            files.push(candidate);
        }
        if !dir.pop() {
            break;
        }
    }

    if files.is_empty() {
        return None;
    }

    // Reverse so root-level files come first, project-level last (overrides)
    files.reverse();

    let mut content = String::from("# Project Instructions\n");
    let mut total_bytes = 0;

    for path in &files {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                if total_bytes + text.len() > MAX_INSTRUCTION_BYTES {
                    let remaining = MAX_INSTRUCTION_BYTES - total_bytes;
                    if remaining > 0 {
                        content.push_str(&text[..remaining]);
                        content.push_str("\n\n[instruction files truncated]");
                    }
                    break;
                }
                content.push('\n');
                content.push_str(&text);
                total_bytes += text.len();
            }
            Err(e) => {
                tracing::warn!("failed to read {}: {e}", path.display());
            }
        }
    }

    Some(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_base_prompt_contains_identity() {
        let prompt = build_system_prompt("claude-sonnet-4-20250514");
        assert!(prompt.contains("helpful AI coding assistant"));
    }

    #[test]
    fn test_environment_context_contains_model() {
        let ctx = environment_context("claude-sonnet-4-20250514");
        assert!(ctx.contains("claude-sonnet-4-20250514"));
    }

    #[test]
    fn test_environment_context_contains_platform() {
        let ctx = environment_context("test-model");
        assert!(ctx.contains(std::env::consts::OS));
    }

    #[test]
    fn test_instruction_file_loaded() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".hob.md"), "Always use snake_case.").unwrap();

        let result = load_instruction_files(dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains("Always use snake_case."));
    }

    #[test]
    fn test_instruction_file_caps_at_limit() {
        let dir = TempDir::new().unwrap();
        let big = "x".repeat(MAX_INSTRUCTION_BYTES + 1000);
        std::fs::write(dir.path().join(".hob.md"), &big).unwrap();

        let result = load_instruction_files(dir.path()).unwrap();
        assert!(result.len() <= MAX_INSTRUCTION_BYTES + 200); // overhead for header + truncation msg
        assert!(result.contains("[instruction files truncated]"));
    }

    #[test]
    fn test_no_instruction_file_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = load_instruction_files(dir.path());
        assert!(result.is_none());
    }
}
