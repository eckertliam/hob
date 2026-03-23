//! glob tool: find files by pattern.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_RESULTS: usize = 100;

#[derive(Deserialize)]
struct Params {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "glob".into(),
        description: "Find files matching a glob pattern. Returns paths sorted by \
                       modification time (newest first). Uses ripgrep if available, \
                       falls back to basic matching."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match (e.g. \"**/*.rs\", \"src/*.ts\")"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in. Defaults to current directory."
                }
            },
            "required": ["pattern"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid glob parameters")?;

    let dir = params.path.as_deref().unwrap_or(".");

    // Try ripgrep first (fast, respects .gitignore)
    let output = tokio::process::Command::new("rg")
        .args(["--files", "--glob", &params.pattern, "--sort", "modified", dir])
        .output()
        .await;

    let lines = match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).to_string()
        }
        _ => {
            // Fallback: use find
            let output = tokio::process::Command::new("find")
                .args([dir, "-name", &params.pattern, "-type", "f"])
                .output()
                .await
                .context("failed to run find")?;
            String::from_utf8_lossy(&output.stdout).to_string()
        }
    };

    let mut results: Vec<&str> = lines.lines().filter(|l| !l.is_empty()).collect();
    let total = results.len();
    results.truncate(MAX_RESULTS);

    let mut output = results.join("\n");
    if total > MAX_RESULTS {
        output.push_str(&format!("\n\n[{total} matches, showing first {MAX_RESULTS}]"));
    }
    if output.is_empty() {
        output.push_str("[no matches]");
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_glob_finds_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();
        std::fs::write(dir.path().join("c.rs"), "").unwrap();

        let result = execute(json!({
            "pattern": "*.txt",
            "path": dir.path().to_str().unwrap()
        }))
        .await
        .unwrap();

        assert!(result.contains("a.txt"));
        assert!(result.contains("b.txt"));
        assert!(!result.contains("c.rs"));
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let dir = TempDir::new().unwrap();
        let result = execute(json!({
            "pattern": "*.nonexistent",
            "path": dir.path().to_str().unwrap()
        }))
        .await
        .unwrap();
        assert_eq!(result, "[no matches]");
    }
}
