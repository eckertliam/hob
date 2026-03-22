//! grep tool: search file contents.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_MATCHES: usize = 100;
const MAX_LINE_CHARS: usize = 2000;

#[derive(Deserialize)]
struct Params {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "grep".into(),
        description: "Search for a regex pattern in files. Returns matching lines with \
                       file paths and line numbers. Uses ripgrep if available."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search. Defaults to current directory."
                }
            },
            "required": ["pattern"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid grep parameters")?;

    let dir = params.path.as_deref().unwrap_or(".");

    let output = tokio::process::Command::new("rg")
        .args([
            "--line-number",
            "--with-filename",
            "--max-count",
            &MAX_MATCHES.to_string(),
            &params.pattern,
            dir,
        ])
        .output()
        .await;

    let raw = match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
        Err(_) => {
            // Fallback to grep
            let out = tokio::process::Command::new("grep")
                .args(["-rn", "--include=*", &params.pattern, dir])
                .output()
                .await
                .context("failed to run grep")?;
            String::from_utf8_lossy(&out.stdout).to_string()
        }
    };

    let mut lines: Vec<String> = raw
        .lines()
        .filter(|l| !l.is_empty())
        .take(MAX_MATCHES)
        .map(|l| {
            if l.len() > MAX_LINE_CHARS {
                format!("{}...", &l[..MAX_LINE_CHARS])
            } else {
                l.to_string()
            }
        })
        .collect();

    let total = raw.lines().filter(|l| !l.is_empty()).count();

    let mut output = lines.join("\n");
    if total > MAX_MATCHES {
        output.push_str(&format!("\n\n[{total} matches, showing first {MAX_MATCHES}]"));
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
    async fn test_grep_finds_pattern() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world\nfoo bar\nhello again\n")
            .unwrap();

        let result = execute(json!({
            "pattern": "hello",
            "path": dir.path().to_str().unwrap()
        }))
        .await
        .unwrap();

        assert!(result.contains("hello"));
        // Should have at least 2 matches
        assert!(result.lines().count() >= 2);
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "nothing here\n").unwrap();

        let result = execute(json!({
            "pattern": "zzzznotfound",
            "path": dir.path().to_str().unwrap()
        }))
        .await
        .unwrap();

        assert_eq!(result, "[no matches]");
    }
}
