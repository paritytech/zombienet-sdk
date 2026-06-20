use clap::{Parser, Subcommand};
use rmcp::{transport::stdio, ServiceExt};
use zombie_mcp::server::ZombienetMcpServer;

mod install;

#[derive(Parser, Debug)]
#[command(name = "zombie-mcp", author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Register zombie-mcp as an MCP server for a client
    Install(install::InstallArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    if let Some(command) = Cli::parse().command {
        match command {
            Command::Install(args) => return install::run(args),
        }
    }

    let service = ZombienetMcpServer::default().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
