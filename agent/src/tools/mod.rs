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
