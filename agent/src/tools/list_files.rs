//! list_files tool: list directory contents.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const MAX_ENTRIES: usize = 500;

#[derive(Deserialize)]
struct Params {
    path: String,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "list_files".into(),
        description: "List files and directories at a given path. \
                       Returns entries sorted alphabetically with a trailing / for directories."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list. Use \".\" for the current directory."
                }
            },
            "required": ["path"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid list_files parameters")?;

    let path = Path::new(&params.path);
    if !path.is_dir() {
        anyhow::bail!("not a directory: {}", params.path);
    }

    let mut entries: Vec<String> = Vec::new();
    let mut read_dir = tokio::fs::read_dir(path)
        .await
        .with_context(|| format!("failed to read directory: {}", params.path))?;

    while let Some(entry) = read_dir.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            entries.push(format!("{name}/"));
        } else {
            entries.push(name);
        }
        if entries.len() >= MAX_ENTRIES {
            break;
        }
    }

    entries.sort();

    let mut output = entries.join("\n");
    if entries.len() >= MAX_ENTRIES {
        output.push_str(&format!("\n\n[truncated at {MAX_ENTRIES} entries]"));
    }

    if output.is_empty() {
        output.push_str("[empty directory]");
    }

    Ok(output)
}
