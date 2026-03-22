//! Anthropic API client.
//!
//! Handles streaming SSE responses from the Messages API.

use anyhow::Result;

pub struct AnthropicClient {
    api_key: String,
    http: reqwest::Client,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::Client::new(),
        }
    }

    /// Send a message and stream back tokens via a channel.
    /// TODO: implement SSE streaming from /v1/messages
    pub async fn stream_message(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
    ) -> Result<tokio::sync::mpsc::Receiver<String>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(32);
        // TODO: implement
        Ok(rx)
    }
}
