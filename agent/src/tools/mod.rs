//! Tool registry and dispatch.
//!
//! Each tool has a name, JSON schema for its parameters, and an async
//! execute function. The registry provides tool definitions (for sending
//! to the LLM) and dispatch (for executing tool calls).

pub mod list_files;
pub mod read_file;
pub mod shell;

use anyhow::Result;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::api::ToolDef;

/// Maximum output size before truncation (50KB).
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Return the list of tool definitions to send to the LLM.
pub fn definitions() -> Vec<ToolDef> {
    vec![
        read_file::definition(),
        shell::definition(),
        list_files::definition(),
    ]
}

/// Execute a named tool with the given input. Returns the output string.
pub async fn execute(
    tool_name: &str,
    input: Value,
    cancel: &CancellationToken,
) -> Result<String> {
    let output = match tool_name {
        "read_file" => read_file::execute(input).await?,
        "shell" => shell::execute(input, cancel).await?,
        "list_files" => list_files::execute(input).await?,
        _ => anyhow::bail!("unknown tool: {tool_name}"),
    };

    Ok(truncate_output(output))
}

/// Truncate tool output if it exceeds the size limit.
fn truncate_output(output: String) -> String {
    if output.len() <= MAX_OUTPUT_BYTES {
        return output;
    }
    let truncated = &output[..MAX_OUTPUT_BYTES];
    // Find the last newline to avoid cutting mid-line
    let cut = truncated.rfind('\n').unwrap_or(MAX_OUTPUT_BYTES);
    format!(
        "{}\n\n[output truncated: {} bytes total, showing first {}]",
        &output[..cut],
        output.len(),
        cut
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_no_op_when_small() {
        let s = "hello world".to_string();
        assert_eq!(truncate_output(s.clone()), s);
    }

    #[test]
    fn test_truncate_large_output() {
        let line = "x".repeat(1000) + "\n";
        // ~1001 bytes per line, need >50KB = >51 lines
        let output: String = line.repeat(60);
        assert!(output.len() > MAX_OUTPUT_BYTES);
        let result = truncate_output(output.clone());
        assert!(result.len() < output.len());
        assert!(result.contains("[output truncated:"));
    }

    #[test]
    fn test_definitions_returns_all_tools() {
        let defs = definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"list_files"));
    }

    #[tokio::test]
    async fn test_dispatch_unknown_tool() {
        let cancel = CancellationToken::new();
        let result = execute("nonexistent", serde_json::json!({}), &cancel).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown tool"));
    }
}
