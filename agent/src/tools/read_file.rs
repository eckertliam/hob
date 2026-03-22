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
