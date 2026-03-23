//! write_file tool: create or overwrite files.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Params {
    path: String,
    content: String,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "write_file".into(),
        description: "Write content to a file. Creates the file and parent directories if \
                       they don't exist. Overwrites the file if it exists."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write"
                }
            },
            "required": ["path", "content"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid write_file parameters")?;

    let path = std::path::Path::new(&params.path);

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }
    }

    tokio::fs::write(&params.path, &params.content)
        .await
        .with_context(|| format!("failed to write file: {}", params.path))?;

    let lines = params.content.lines().count();
    let mut output = format!("Wrote {} lines to {}", lines, params.path);

    let diags = crate::lsp::check_file(&params.path);
    if !diags.is_empty() {
        output.push_str("\n\nDiagnostics:\n");
        for d in &diags {
            output.push_str(&format!("  {d}\n"));
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        let result = execute(json!({
            "path": path.to_str().unwrap(),
            "content": "hello\nworld"
        }))
        .await
        .unwrap();
        assert!(result.contains("2 lines"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\nworld");
    }

    #[tokio::test]
    async fn test_write_creates_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a/b/c/test.txt");
        let result = execute(json!({
            "path": path.to_str().unwrap(),
            "content": "nested"
        }))
        .await;
        assert!(result.is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "nested");
    }

    #[tokio::test]
    async fn test_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "old").unwrap();
        execute(json!({
            "path": path.to_str().unwrap(),
            "content": "new"
        }))
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }
}
