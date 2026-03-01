use anyhow::Result;
use async_bash_mcp::server::AsyncBashServer;
use rmcp::{ServiceExt, transport::io::stdio};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let service = AsyncBashServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
