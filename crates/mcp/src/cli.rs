use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use zombie_mcp::{
    diagnostics,
    input::{ConfigInput, DiagnoseRunInput},
    recent_runs,
    report::Status,
};

#[derive(Args, Debug)]
pub struct DiagnoseArgs {
    /// Diagnose a specific run by its zombie.json path.
    #[arg(long, conflicts_with = "auto")]
    zombie_json: Option<PathBuf>,
    /// Auto-discover the most recent run and diagnose it.
    #[arg(long)]
    auto: bool,
    /// Exit with code 1 when the report status is `failed`.
    #[arg(long)]
    fail_on_error: bool,
}

#[derive(Args, Debug)]
pub struct ValidateArgs {
    /// Path to the Zombienet configuration file (.toml) to validate.
    #[arg(long)]
    config: PathBuf,
    /// Exit with code 1 when the report status is `failed`.
    #[arg(long)]
    fail_on_error: bool,
}

/// Run read-only diagnostics without an MCP/LLM client and print a JSON report.
///
/// This is the built-in orchestration the LLM used to perform by hand: resolve a
/// run path (explicitly or via auto-discovery), run the same `diagnose_run` core
/// the MCP tool calls, print the report to stdout, and optionally gate CI on it.
pub async fn run(args: DiagnoseArgs) -> anyhow::Result<()> {
    let path = resolve_run_path(args.zombie_json, args.auto)?;
    let report = diagnostics::diagnose_run(DiagnoseRunInput {
        zombie_json_path: path,
    })
    .await;

    let json = serde_json::to_string(&report).context("diagnostic reports should serialize")?;
    println!("{json}");

    if args.fail_on_error && report.status == Status::Failed {
        std::process::exit(1);
    }

    Ok(())
}

/// Validate a Zombienet config file without an MCP/LLM client and print a JSON
/// report. Calls the same `validate_config` core the MCP tool exposes.
pub fn validate(args: ValidateArgs) -> anyhow::Result<()> {
    let report = diagnostics::validate_config(ConfigInput {
        config_path: args.config,
    });

    let json = serde_json::to_string(&report).context("diagnostic reports should serialize")?;
    println!("{json}");

    if args.fail_on_error && report.status == Status::Failed {
        std::process::exit(1);
    }

    Ok(())
}

fn resolve_run_path(zombie_json: Option<PathBuf>, auto: bool) -> anyhow::Result<PathBuf> {
    match (zombie_json, auto) {
        (Some(path), _) => Ok(path),
        (None, true) => pick_newest_run(),
        (None, false) => anyhow::bail!("pass --zombie-json <path> or --auto"),
    }
}

/// Pick the newest run discovered by `find_recent_runs`.
///
/// Runs are already sorted newest-first, so this is the `find_recent_runs ->
/// diagnose_run` sequence the LLM used to drive, now built in.
fn pick_newest_run() -> anyhow::Result<PathBuf> {
    let report = recent_runs::find_recent_runs();
    let newest = report
        .runs
        .into_iter()
        .next()
        .context("no recent zombienet runs found; pass --zombie-json <path>")?;
    Ok(PathBuf::from(newest.zombie_json_path))
}
