//! shell tool: execute shell commands.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const DEFAULT_TIMEOUT_MS: u64 = 120_000; // 2 minutes

#[derive(Deserialize)]
struct Params {
    command: String,
    #[serde(default)]
    timeout: Option<u64>,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "shell".into(),
        description: "Execute a shell command and return its stdout and stderr. \
                       Commands run in the working directory. \
                       Default timeout is 120 seconds."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds. Defaults to 120000 (2 minutes)."
                }
            },
            "required": ["command"]
        }),
    }
}

pub async fn execute(input: Value, cancel: &CancellationToken) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid shell parameters")?;

    let timeout_ms = params.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let timeout = std::time::Duration::from_millis(timeout_ms);

    let child = Command::new("sh")
        .arg("-c")
        .arg(&params.command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to spawn: {}", params.command))?;

    // Wait for completion, timeout, or cancellation.
    // wait_with_output takes ownership, so wrap in an async block
    // that we can race against timeout/cancel.
    let output = tokio::select! {
        result = child.wait_with_output() => {
            result.context("failed to wait for process")?
        }
        _ = tokio::time::sleep(timeout) => {
            // kill_on_drop handles cleanup when `child` is dropped here
            anyhow::bail!(
                "command timed out after {}s: {}",
                timeout_ms / 1000,
                params.command
            );
        }
        _ = cancel.cancelled() => {
            anyhow::bail!("command cancelled: {}", params.command);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit_code = output.status.code().unwrap_or(-1);

    let mut result = String::new();

    if !stdout.is_empty() {
        result.push_str(&stdout);
    }

    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("[stderr]\n");
        result.push_str(&stderr);
    }

    if !output.status.success() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!("[exit code: {exit_code}]"));
    }

    if result.is_empty() {
        result.push_str("[no output]");
    }

    Ok(result)
}
