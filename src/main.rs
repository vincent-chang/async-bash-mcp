use anyhow::Result;
use async_bash_mcp::server::AsyncBashServer;
use rmcp::{transport::io::stdio, ServiceExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let logging_enabled = std::env::args().any(|a| a == "--log");
    let service = AsyncBashServer::new(logging_enabled).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
