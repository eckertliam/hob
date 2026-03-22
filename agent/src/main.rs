mod agent;
mod api;
mod compaction;
mod config;
mod error;
mod events;
mod models;
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

    let cfg = config::Config::load().unwrap_or_default();
    let model = cfg.resolve_model();

    let provider: Arc<dyn api::Provider> = match build_provider(&cfg)? {
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

fn build_provider(cfg: &config::Config) -> Result<ProviderKind> {
    let forced = cfg.resolve_provider();
    let base_url = cfg.resolve_base_url();

    match forced.as_deref() {
        Some("openai") => {
            let key = cfg
                .resolve_api_key("openai")
                .context("Provider is openai but no OPENAI_API_KEY set.\nRun: /key openai <your-key>")?;
            Ok(ProviderKind::OpenAI(key, base_url))
        }
        Some("anthropic") => {
            let key = cfg
                .resolve_api_key("anthropic")
                .context("Provider is anthropic but no ANTHROPIC_API_KEY set.\nRun: /key anthropic <your-key>")?;
            Ok(ProviderKind::Anthropic(key))
        }
        Some(other) => {
            anyhow::bail!("unknown provider: {other} (expected \"anthropic\" or \"openai\")");
        }
        None => {
            // Auto-detect from available keys
            if let Some(key) = cfg.resolve_api_key("anthropic") {
                return Ok(ProviderKind::Anthropic(key));
            }
            if let Some(key) = cfg.resolve_api_key("openai") {
                return Ok(ProviderKind::OpenAI(key, base_url));
            }
            anyhow::bail!(
                "No API key found.\n\n\
                 Set an environment variable:\n  \
                 export ANTHROPIC_API_KEY=sk-ant-...\n  \
                 export OPENAI_API_KEY=sk-...\n\n\
                 Or run hob and use:\n  \
                 /key anthropic sk-ant-...\n  \
                 /key openai sk-..."
            );
        }
    }
}
