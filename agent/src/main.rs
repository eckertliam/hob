mod agent;
mod api;
mod ipc;
mod tools;

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Log to stderr only — stdout is reserved for IPC with Emacs
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    info!("hob-agent starting");

    ipc::run_loop().await?;

    Ok(())
}
