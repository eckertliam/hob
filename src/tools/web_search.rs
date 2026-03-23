//! web_search tool: search the web and return results.
//!
//! Uses DuckDuckGo HTML search to avoid API key requirements.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_RESULTS: usize = 10;

#[derive(Deserialize)]
struct Params {
    query: String,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "web_search".into(),
        description: "Search the web and return results. Returns titles, URLs, \
                       and snippets from DuckDuckGo."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            },
            "required": ["query"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid web_search parameters")?;

    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoded(&params.query)
    );

    let body = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "hob/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("search request failed")?
        .text()
        .await
        .context("failed to read search response")?;

    let results = parse_ddg_html(&body);

    if results.is_empty() {
        return Ok(format!("No results found for: {}", params.query));
    }

    let mut output = format!("Search results for: {}\n\n", params.query);
    for (i, (title, url, snippet)) in results.iter().take(MAX_RESULTS).enumerate() {
        output.push_str(&format!("{}. {}\n   {}\n   {}\n\n", i + 1, title, url, snippet));
    }

    Ok(output)
}

/// Simple URL encoding for the query string.
fn urlencoded(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => '+'.to_string(),
            c if c.is_alphanumeric() || "-._~".contains(c) => c.to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

/// Parse DuckDuckGo HTML search results.
/// Extracts title, URL, and snippet from result divs.
fn parse_ddg_html(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();

    // DuckDuckGo HTML results are in <a class="result__a"> tags
    for chunk in html.split("class=\"result__a\"") {
        if results.len() >= MAX_RESULTS {
            break;
        }
        if chunk.len() < 10 {
            continue;
        }

        // Extract URL from href
        let url = extract_between(chunk, "href=\"", "\"")
            .unwrap_or_default()
            .to_string();
        if url.is_empty() || url.starts_with('/') {
            continue;
        }

        // Extract title (text inside the <a> tag)
        let title = extract_between(chunk, ">", "</a>")
            .map(|s| strip_html_tags(s))
            .unwrap_or_default();

        // Extract snippet from result__snippet
        let snippet = if let Some(rest) = chunk.split("result__snippet").nth(1) {
            extract_between(rest, ">", "</")
                .map(|s| strip_html_tags(s))
                .unwrap_or_default()
        } else {
            String::new()
        };

        if !title.is_empty() {
            results.push((title, url, snippet));
        }
    }

    results
}

fn extract_between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = s.find(start)? + start.len();
    let end_idx = s[start_idx..].find(end)? + start_idx;
    Some(&s[start_idx..end_idx])
}

fn strip_html_tags(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition() {
        let def = definition();
        assert_eq!(def.name, "web_search");
    }

    #[test]
    fn test_urlencoded() {
        assert_eq!(urlencoded("hello world"), "hello+world");
        assert_eq!(urlencoded("rust lang"), "rust+lang");
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<b>hello</b>"), "hello");
        assert_eq!(strip_html_tags("a &amp; b"), "a & b");
    }

    #[test]
    fn test_extract_between() {
        assert_eq!(extract_between("foo=bar&baz", "=", "&"), Some("bar"));
        assert_eq!(extract_between("nothing", "x", "y"), None);
    }
}
