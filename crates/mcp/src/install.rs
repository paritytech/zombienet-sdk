use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use clap::{Args, Subcommand};

const DEFAULT_SERVER_NAME: &str = "zombie-mcp";

#[derive(Args, Debug)]
pub struct InstallArgs {
    #[command(subcommand)]
    client: Client,
}

#[derive(Subcommand, Debug)]
enum Client {
    /// Register zombie-mcp with the Codex CLI
    Codex(CodexArgs),
    /// Register zombie-mcp with the Claude Code CLI
    Claude(ClaudeArgs),
}

/// Options shared by every MCP client registration.
#[derive(Args, Debug)]
struct CommonArgs {
    /// MCP server name to register
    #[arg(long, default_value = DEFAULT_SERVER_NAME)]
    name: String,
    /// Path to the zombie-mcp binary (defaults to the current executable)
    #[arg(long)]
    bin: Option<PathBuf>,
    /// Remove any existing registration before adding
    #[arg(long)]
    force: bool,
}

#[derive(Args, Debug)]
struct CodexArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Codex executable to invoke
    #[arg(long, default_value = "codex")]
    codex_bin: OsString,
}

#[derive(Args, Debug)]
struct ClaudeArgs {
    #[command(flatten)]
    common: CommonArgs,
    /// Claude Code executable to invoke
    #[arg(long, default_value = "claude")]
    claude_bin: OsString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientKind {
    Codex,
    Claude,
}

impl ClientKind {
    /// Human-readable name used in the success messages.
    fn label(self) -> &'static str {
        match self {
            ClientKind::Codex => "Codex",
            ClientKind::Claude => "Claude Code",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallOptions {
    kind: ClientKind,
    name: String,
    server_bin: PathBuf,
    client_bin: OsString,
    force: bool,
}

pub fn run(args: InstallArgs) -> anyhow::Result<()> {
    let options = match args.client {
        Client::Codex(args) => {
            InstallOptions::resolve(ClientKind::Codex, args.common, args.codex_bin)?
        },
        Client::Claude(args) => {
            InstallOptions::resolve(ClientKind::Claude, args.common, args.claude_bin)?
        },
    };

    install_client(&options)
}

fn install_client(options: &InstallOptions) -> anyhow::Result<()> {
    if options.force {
        let remove_args = [
            OsString::from("mcp"),
            OsString::from("remove"),
            OsString::from(&options.name),
        ];
        run_client(options, &remove_args, true)?;
    }

    let add_args = [
        OsString::from("mcp"),
        OsString::from("add"),
        OsString::from(&options.name),
        OsString::from("--"),
        options.server_bin.as_os_str().to_os_string(),
    ];
    run_client(options, &add_args, false)?;

    println!(
        "Installed `{}` as a {} MCP server using {}",
        options.name,
        options.kind.label(),
        options.server_bin.display(),
    );
    println!(
        "Start a new {} session to load the MCP server",
        options.kind.label(),
    );
    Ok(())
}

fn run_client(
    options: &InstallOptions,
    args: &[OsString],
    allow_failure: bool,
) -> anyhow::Result<()> {
    let status = Command::new(&options.client_bin).args(args).status()?;
    if !status.success() && !allow_failure {
        anyhow::bail!(
            "{} command failed with status {status}",
            options.kind.label()
        );
    }

    Ok(())
}

impl InstallOptions {
    fn resolve(kind: ClientKind, common: CommonArgs, client_bin: OsString) -> anyhow::Result<Self> {
        if common.name.trim().is_empty() {
            anyhow::bail!("--name cannot be empty");
        }

        let server_bin = match common.bin {
            Some(bin) => bin,
            None => default_server_bin()?,
        };

        Ok(Self {
            kind,
            name: common.name,
            server_bin,
            client_bin,
            force: common.force,
        })
    }
}

fn default_server_bin() -> anyhow::Result<PathBuf> {
    let current_exe = env::current_exe()?;
    absolute_path(&current_exe)
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(env::current_dir()?.join(path))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(subcommand)]
        client: Client,
    }

    fn parse(args: &[&str]) -> anyhow::Result<InstallOptions> {
        let cli = TestCli::try_parse_from(std::iter::once("install").chain(args.iter().copied()))?;
        match cli.client {
            Client::Codex(args) => {
                InstallOptions::resolve(ClientKind::Codex, args.common, args.codex_bin)
            },
            Client::Claude(args) => {
                InstallOptions::resolve(ClientKind::Claude, args.common, args.claude_bin)
            },
        }
    }

    #[test]
    fn parse_codex_defaults_to_zombie_mcp_name() {
        let options = parse(&["codex"]).unwrap();

        assert_eq!(options.kind, ClientKind::Codex);
        assert_eq!(options.name, DEFAULT_SERVER_NAME);
        assert_eq!(options.client_bin, OsString::from("codex"));
        assert!(!options.force);
    }

    #[test]
    fn parse_codex_customizes_options() {
        let options = parse(&[
            "codex",
            "--name",
            "local-zombie",
            "--bin",
            "/tmp/zombie-mcp",
            "--codex-bin",
            "/tmp/codex",
            "--force",
        ])
        .unwrap();

        assert_eq!(options.name, "local-zombie");
        assert_eq!(options.server_bin, PathBuf::from("/tmp/zombie-mcp"));
        assert_eq!(options.client_bin, OsString::from("/tmp/codex"));
        assert!(options.force);
    }

    #[test]
    fn parse_claude_defaults_to_claude_bin() {
        let options = parse(&["claude"]).unwrap();

        assert_eq!(options.kind, ClientKind::Claude);
        assert_eq!(options.name, DEFAULT_SERVER_NAME);
        assert_eq!(options.client_bin, OsString::from("claude"));
        assert!(!options.force);
    }

    #[test]
    fn parse_claude_customizes_options() {
        let options = parse(&[
            "claude",
            "--name",
            "local-zombie",
            "--bin",
            "/tmp/zombie-mcp",
            "--claude-bin",
            "/tmp/claude",
            "--force",
        ])
        .unwrap();

        assert_eq!(options.kind, ClientKind::Claude);
        assert_eq!(options.name, "local-zombie");
        assert_eq!(options.server_bin, PathBuf::from("/tmp/zombie-mcp"));
        assert_eq!(options.client_bin, OsString::from("/tmp/claude"));
        assert!(options.force);
    }

    #[test]
    fn parse_rejects_empty_name() {
        let error = parse(&["codex", "--name", ""]).unwrap_err();

        assert!(error.to_string().contains("--name cannot be empty"));
    }
}
