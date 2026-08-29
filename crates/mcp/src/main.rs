use clap::{Parser, Subcommand};

mod cli;
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
    /// Run read-only diagnostics without an MCP/LLM client and print a JSON report.
    Diagnose(cli::DiagnoseArgs),
    /// Validate a Zombienet config file without an MCP/LLM client.
    Validate(cli::ValidateArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,zombienet_orchestrator=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    if let Some(command) = Cli::parse().command {
        match command {
            Command::Install(args) => return install::run(args),
            Command::Diagnose(args) => return cli::run(args).await,
            Command::Validate(args) => return cli::validate(args),
        }
    }

    run_default().await
}

/// With the `mcp` feature, the default (no subcommand) launches the MCP stdio server.
#[cfg(feature = "mcp")]
async fn run_default() -> anyhow::Result<()> {
    use rmcp::{transport::stdio, ServiceExt};
    use zombie_mcp::server::ZombienetMcpServer;

    let service = ZombienetMcpServer::default().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Without the `mcp` feature there is no server, so point the user at the CLI.
#[cfg(not(feature = "mcp"))]
async fn run_default() -> anyhow::Result<()> {
    use clap::CommandFactory;

    Cli::command().print_help()?;
    eprintln!();
    eprintln!("This build has no MCP server (compiled with --no-default-features).");
    eprintln!("Run `zombie-mcp diagnose --auto` for read-only diagnostics.");
    Ok(())
}
