mod agent;
mod api;
mod error;
mod ipc;
mod permission;
mod prompt;
mod store;
mod tools;

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Log to stderr only — stdout is reserved for IPC with Emacs
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    info!("hob-agent starting");

    let api_key =
        std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;
    let model =
        std::env::var("HOB_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".into());

    let provider: Arc<dyn api::Provider> =
        Arc::new(api::anthropic::AnthropicProvider::new(api_key));

    let db_path = store::Store::default_path();
    let store = store::Store::open(&db_path)
        .with_context(|| format!("failed to open store at {}", db_path.display()))?;
    info!("store opened at {}", db_path.display());

    ipc::run_loop(provider, model, store).await?;

    Ok(())
}
