//! edit_file tool: find-and-replace with fuzzy matching cascade.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Params {
    path: String,
    old_text: String,
    new_text: String,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "edit_file".into(),
        description: "Edit a file by replacing old_text with new_text. Uses fuzzy matching \
                       if an exact match isn't found (whitespace-normalized, then \
                       indentation-flexible)."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "The text to find and replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "The replacement text"
                }
            },
            "required": ["path", "old_text", "new_text"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid edit_file parameters")?;

    let content = tokio::fs::read_to_string(&params.path)
        .await
        .with_context(|| format!("failed to read file: {}", params.path))?;

    let (new_content, method) = apply_edit(&content, &params.old_text, &params.new_text)?;

    tokio::fs::write(&params.path, &new_content)
        .await
        .with_context(|| format!("failed to write file: {}", params.path))?;

    Ok(format!("Edited {} (matched via {method})", params.path))
}

/// Try to apply the edit using a cascade of matching strategies.
/// Returns (new_content, method_name) or an error if no match found.
fn apply_edit(content: &str, old_text: &str, new_text: &str) -> Result<(String, &'static str)> {
    // 1. Exact match
    if let Some(pos) = content.find(old_text) {
        let mut result = String::with_capacity(content.len());
        result.push_str(&content[..pos]);
        result.push_str(new_text);
        result.push_str(&content[pos + old_text.len()..]);
        return Ok((result, "exact match"));
    }

    // 2. Whitespace-normalized match
    if let Some(result) = whitespace_normalized_replace(content, old_text, new_text) {
        return Ok((result, "whitespace-normalized"));
    }

    // 3. Indentation-flexible match
    if let Some(result) = indentation_flexible_replace(content, old_text, new_text) {
        return Ok((result, "indentation-flexible"));
    }

    // 4. Context-anchored match (first and last lines as anchors)
    if let Some(result) = context_anchored_replace(content, old_text, new_text) {
        return Ok((result, "context-anchored"));
    }

    anyhow::bail!(
        "Could not find text to replace. The old_text was not found in the file \
         (tried exact, whitespace-normalized, indentation-flexible, and context-anchored matching)."
    )
}

/// Normalize whitespace: collapse runs of whitespace to a single space.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Try whitespace-normalized matching: collapse whitespace in both the
/// search text and content, find the match, then replace the corresponding
/// region in the original content.
fn whitespace_normalized_replace(content: &str, old_text: &str, new_text: &str) -> Option<String> {
    let norm_old = normalize_ws(old_text);
    let norm_content = normalize_ws(content);

    let norm_pos = norm_content.find(&norm_old)?;

    // Map normalized position back to original content position.
    // Walk through content, tracking normalized position.
    let (start, end) = map_normalized_range(content, norm_pos, norm_old.len())?;

    let mut result = String::with_capacity(content.len());
    result.push_str(&content[..start]);
    result.push_str(new_text);
    result.push_str(&content[end..]);
    Some(result)
}

/// Map a position range in normalized text back to the original text.
fn map_normalized_range(content: &str, norm_start: usize, norm_len: usize) -> Option<(usize, usize)> {
    let mut norm_pos = 0;
    let mut orig_start = None;
    let mut in_ws = false;
    let bytes = content.as_bytes();
    let mut i = 0;

    // Skip leading whitespace in normalized content accounting
    while i < bytes.len() {
        let b = bytes[i];
        let is_ws = b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';

        if is_ws {
            if !in_ws {
                if norm_pos > 0 {
                    norm_pos += 1; // the single space that collapsed whitespace maps to
                }
                in_ws = true;
            }
        } else {
            in_ws = false;
            if orig_start.is_none() && norm_pos >= norm_start {
                orig_start = Some(i);
            }
            norm_pos += 1;
            if orig_start.is_some() && norm_pos >= norm_start + norm_len {
                return Some((orig_start.unwrap(), i + 1));
            }
        }
        i += 1;
    }

    // Handle case where match extends to end of content
    if let Some(start) = orig_start {
        if norm_pos >= norm_start + norm_len {
            return Some((start, content.len()));
        }
    }

    None
}

/// Try indentation-flexible matching: strip leading whitespace from each line
/// in both texts, find the match, replace the corresponding lines.
fn indentation_flexible_replace(content: &str, old_text: &str, new_text: &str) -> Option<String> {
    let content_lines: Vec<&str> = content.lines().collect();
    let old_lines: Vec<&str> = old_text.lines().collect();

    if old_lines.is_empty() {
        return None;
    }

    let stripped_old: Vec<&str> = old_lines.iter().map(|l| l.trim_start()).collect();

    // Slide the window over content lines
    for start in 0..=content_lines.len().saturating_sub(old_lines.len()) {
        let window = &content_lines[start..start + old_lines.len()];
        let stripped_window: Vec<&str> = window.iter().map(|l| l.trim_start()).collect();

        if stripped_window == stripped_old {
            // Match found. Rebuild content with replacement.
            let mut result = String::new();
            for line in &content_lines[..start] {
                result.push_str(line);
                result.push('\n');
            }
            result.push_str(new_text);
            if !new_text.ends_with('\n') {
                result.push('\n');
            }
            for line in &content_lines[start + old_lines.len()..] {
                result.push_str(line);
                result.push('\n');
            }
            // Preserve original trailing newline behavior
            if !content.ends_with('\n') && result.ends_with('\n') {
                result.pop();
            }
            return Some(result);
        }
    }

    None
}

/// Try context-anchored matching: use first and last lines of old_text as
/// anchors, replace everything between them.
fn context_anchored_replace(content: &str, old_text: &str, new_text: &str) -> Option<String> {
    let old_lines: Vec<&str> = old_text.lines().collect();
    if old_lines.len() < 2 {
        return None;
    }

    let first = old_lines.first()?.trim();
    let last = old_lines.last()?.trim();
    if first.is_empty() || last.is_empty() {
        return None;
    }

    let content_lines: Vec<&str> = content.lines().collect();

    // Find first anchor
    let start = content_lines
        .iter()
        .position(|l| l.trim() == first)?;

    // Find last anchor after start
    let end = content_lines[start..]
        .iter()
        .rposition(|l| l.trim() == last)
        .map(|i| start + i)?;

    if end < start {
        return None;
    }

    let mut result = String::new();
    for line in &content_lines[..start] {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(new_text);
    if !new_text.ends_with('\n') {
        result.push('\n');
    }
    for line in &content_lines[end + 1..] {
        result.push_str(line);
        result.push('\n');
    }
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_exact_match_replace() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.rs");
        std::fs::write(&path, "fn foo() {\n    old\n}\n").unwrap();
        let result = execute(json!({
            "path": path.to_str().unwrap(),
            "old_text": "    old",
            "new_text": "    new"
        }))
        .await
        .unwrap();
        assert!(result.contains("exact match"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("    new"));
        assert!(!content.contains("    old"));
    }

    #[tokio::test]
    async fn test_indentation_flexible_match() {
        // Test the indentation_flexible_replace function directly
        let content = "impl Foo {\n        fn bar(&self) {\n            do_thing();\n        }\n}";
        let old_text = "fn bar(&self) {\n    do_thing();\n}";
        let new_text = "fn baz(&self) {\n    do_other();\n}";

        let result = indentation_flexible_replace(content, old_text, new_text);
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.contains("do_other"));
        assert!(!result.contains("do_thing"));
    }

    #[tokio::test]
    async fn test_context_anchored_match() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.rs");
        std::fs::write(
            &path,
            "fn main() {\n    let x = 1;\n    let y = 2;\n    let z = 3;\n}\n",
        )
        .unwrap();
        // Anchors: "fn main() {" and "}" but middle lines differ slightly
        let result = execute(json!({
            "path": path.to_str().unwrap(),
            "old_text": "fn main() {\n    let x = 1;\n    let y = 99;\n    let z = 3;\n}",
            "new_text": "fn main() {\n    println!(\"hello\");\n}"
        }))
        .await
        .unwrap();
        assert!(result.contains("context-anchored"));
    }

    #[tokio::test]
    async fn test_no_match_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.rs");
        std::fs::write(&path, "fn foo() {}").unwrap();
        let result = execute(json!({
            "path": path.to_str().unwrap(),
            "old_text": "completely different text",
            "new_text": "replacement"
        }))
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_normalize_ws() {
        assert_eq!(normalize_ws("  a   b  c  "), "a b c");
        assert_eq!(normalize_ws("no\n  change"), "no change");
    }

    #[test]
    fn test_apply_edit_exact() {
        let (result, method) = apply_edit("hello world", "world", "rust").unwrap();
        assert_eq!(result, "hello rust");
        assert_eq!(method, "exact match");
    }
}
