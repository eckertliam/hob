//! web_fetch tool: fetch a URL and return its content.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_BODY_BYTES: usize = 100_000;

#[derive(Deserialize)]
struct Params {
    url: String,
}

pub fn definition() -> crate::api::ToolDef {
    crate::api::ToolDef {
        name: "web_fetch".into(),
        description: "Fetch a URL and return its text content. \
                       Useful for reading documentation, APIs, or web pages."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                }
            },
            "required": ["url"]
        }),
    }
}

pub async fn execute(input: Value) -> Result<String> {
    let params: Params =
        serde_json::from_value(input).context("invalid web_fetch parameters")?;

    let response = reqwest::Client::new()
        .get(&params.url)
        .header("User-Agent", "hob/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .with_context(|| format!("failed to fetch: {}", params.url))?;

    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let body = response
        .text()
        .await
        .with_context(|| format!("failed to read response from: {}", params.url))?;

    let truncated = if body.len() > MAX_BODY_BYTES {
        format!(
            "{}\n\n[truncated: {} bytes total, showing first {}]",
            &body[..MAX_BODY_BYTES],
            body.len(),
            MAX_BODY_BYTES
        )
    } else {
        body
    };

    Ok(format!(
        "HTTP {} | {}\n\n{}",
        status.as_u16(),
        content_type,
        truncated
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition_has_url_param() {
        let def = definition();
        assert_eq!(def.name, "web_fetch");
        let schema = def.input_schema;
        assert!(schema.pointer("/properties/url").is_some());
    }

    #[tokio::test]
    async fn test_invalid_url_returns_error() {
        let result = execute(json!({"url": "not-a-url"})).await;
        assert!(result.is_err());
    }
}
