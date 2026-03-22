//! read_file tool: read file contents with line numbers.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ToolDef;

const DEFAULT_LIMIT: usize = 2000;
const MAX_LINE_CHARS: usize = 2000;

#[derive(Deserialize)]
struct Params {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

pub fn definition() -> ToolDef {
    ToolDef {
        name: "read_file".into(),
        description: "Read a file's contents. Returns lines with line numbers. \
                       Use offset and limit for large files."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "offset": {
                    "type": "integer",
                    "description": "Start reading from this line number (1-based). Defaults to 1."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read. Defaults to 2000."
                }
            },
            "required": ["path"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid read_file parameters")?;

    let content = tokio::fs::read_to_string(&params.path)
        .await
        .with_context(|| format!("failed to read file: {}", params.path))?;

    let offset = params.offset.unwrap_or(1).max(1);
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT);

    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let start = (offset - 1).min(total);
    let end = (start + limit).min(total);

    let mut output = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let line_num = start + i + 1;
        let display_line = if line.len() > MAX_LINE_CHARS {
            format!("{}...", &line[..MAX_LINE_CHARS])
        } else {
            line.to_string()
        };
        output.push_str(&format!("{:>6}\t{}\n", line_num, display_line));
    }

    if end < total {
        output.push_str(&format!(
            "\n[showing lines {}-{} of {}]\n",
            start + 1,
            end,
            total
        ));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn temp_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[tokio::test]
    async fn test_read_whole_file() {
        let f = temp_file("line1\nline2\nline3\n");
        let result = execute(json!({"path": f.path().to_str().unwrap()}))
            .await
            .unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
        // Check line numbers
        assert!(result.contains("     1\t"));
        assert!(result.contains("     2\t"));
        assert!(result.contains("     3\t"));
    }

    #[tokio::test]
    async fn test_read_with_offset_and_limit() {
        let content = (1..=10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let f = temp_file(&content);
        let result = execute(json!({
            "path": f.path().to_str().unwrap(),
            "offset": 3,
            "limit": 2
        }))
        .await
        .unwrap();
        assert!(result.contains("line3"));
        assert!(result.contains("line4"));
        assert!(!result.contains("line2"));
        assert!(!result.contains("line5"));
        assert!(result.contains("[showing lines 3-4 of 10]"));
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let result = execute(json!({"path": "/tmp/hob_nonexistent_file_test"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_long_lines_truncated() {
        let long_line = "x".repeat(3000);
        let f = temp_file(&long_line);
        let result = execute(json!({"path": f.path().to_str().unwrap()}))
            .await
            .unwrap();
        // Should be truncated with ...
        assert!(result.contains("..."));
        assert!(result.len() < 3000 + 100); // line number overhead
    }
}
