//! Tool dispatch and implementations.
//!
//! Each tool corresponds to a capability the agent can invoke
//! (read file, write file, run shell command, etc.)

use anyhow::Result;
use serde_json::Value;

/// Execute a named tool with the given input, return its output string.
/// TODO: implement each tool
pub async fn execute(tool_name: &str, input: Value) -> Result<String> {
    match tool_name {
        "read_file" => read_file(input).await,
        "write_file" => write_file(input).await,
        "shell" => shell(input).await,
        "list_files" => list_files(input).await,
        _ => anyhow::bail!("Unknown tool: {tool_name}"),
    }
}

async fn read_file(_input: Value) -> Result<String> {
    // TODO: implement
    todo!("read_file")
}

async fn write_file(_input: Value) -> Result<String> {
    // TODO: implement
    todo!("write_file")
}

async fn shell(_input: Value) -> Result<String> {
    // TODO: implement
    todo!("shell")
}

async fn list_files(_input: Value) -> Result<String> {
    // TODO: implement
    todo!("list_files")
}
