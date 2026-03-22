mod agent;
mod api;
mod compaction;
mod error;
mod events;
mod permission;
mod prompt;
mod store;
mod tools;
mod tui;

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Log to a file — stderr is used by the TUI
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hob.log")
        .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(log_file))
        .init();

    info!("hob starting");

    let model =
        std::env::var("HOB_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".into());

    let provider: Arc<dyn api::Provider> = match detect_provider()? {
        ProviderKind::Anthropic(key) => {
            info!("using Anthropic provider");
            Arc::new(api::anthropic::AnthropicProvider::new(key))
        }
        ProviderKind::OpenAI(key, base_url) => {
            info!("using OpenAI provider");
            if let Some(url) = base_url {
                Arc::new(api::openai::OpenAIProvider::with_base_url(key, url))
            } else {
                Arc::new(api::openai::OpenAIProvider::new(key))
            }
        }
    };

    let db_path = store::Store::default_path();
    let store = store::Store::open(&db_path)
        .with_context(|| format!("failed to open store at {}", db_path.display()))?;
    info!("store opened at {}", db_path.display());

    tui::run(provider, model, store).await?;

    Ok(())
}

enum ProviderKind {
    Anthropic(String),
    OpenAI(String, Option<String>),
}

fn detect_provider() -> Result<ProviderKind> {
    let forced = std::env::var("HOB_PROVIDER").ok();

    match forced.as_deref() {
        Some("openai") => {
            let key = std::env::var("OPENAI_API_KEY")
                .context("HOB_PROVIDER=openai but OPENAI_API_KEY not set")?;
            let base_url = std::env::var("OPENAI_API_BASE").ok();
            Ok(ProviderKind::OpenAI(key, base_url))
        }
        Some("anthropic") => {
            let key = std::env::var("ANTHROPIC_API_KEY")
                .context("HOB_PROVIDER=anthropic but ANTHROPIC_API_KEY not set")?;
            Ok(ProviderKind::Anthropic(key))
        }
        Some(other) => {
            anyhow::bail!("unknown HOB_PROVIDER: {other} (expected \"anthropic\" or \"openai\")");
        }
        None => {
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                return Ok(ProviderKind::Anthropic(key));
            }
            if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                let base_url = std::env::var("OPENAI_API_BASE").ok();
                return Ok(ProviderKind::OpenAI(key, base_url));
            }
            anyhow::bail!(
                "No API key found. Set ANTHROPIC_API_KEY or OPENAI_API_KEY.\n\
                 You can also set HOB_PROVIDER to force a provider."
            );
        }
    }
}
