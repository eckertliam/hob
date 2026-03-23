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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_directory() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let result = execute(json!({"path": dir.path().to_str().unwrap()}))
            .await
            .unwrap();

        let lines: Vec<&str> = result.lines().collect();
        // Should be sorted: a.txt, b.txt, subdir/
        assert_eq!(lines[0], "a.txt");
        assert_eq!(lines[1], "b.txt");
        assert_eq!(lines[2], "subdir/");
    }

    #[tokio::test]
    async fn test_list_empty_directory() {
        let dir = TempDir::new().unwrap();
        let result = execute(json!({"path": dir.path().to_str().unwrap()}))
            .await
            .unwrap();
        assert_eq!(result, "[empty directory]");
    }

    #[tokio::test]
    async fn test_list_nonexistent_path() {
        let result = execute(json!({"path": "/tmp/hob_nonexistent_dir_test"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_file_not_dir() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let result = execute(json!({"path": f.path().to_str().unwrap()})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }
}
