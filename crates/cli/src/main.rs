use std::{
    fs,
    time::{Duration, Instant},
};

use clap::{Args as ClapArgs, Parser, Subcommand};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use zombienet_sdk::{environment::Provider, GlobalSettingsBuilder, NetworkConfig};

#[derive(Debug, PartialEq, Eq)]
pub enum NodeVerifier {
    None,
    Metric,
}

impl<T: AsRef<str>> From<T> for NodeVerifier {
    fn from(value: T) -> Self {
        match value.as_ref().to_ascii_lowercase().as_ref() {
            "none" => NodeVerifier::None,
            _ => NodeVerifier::Metric, // default
        }
    }
}

/// Common options for spawning networks
#[derive(ClapArgs, Debug, Clone)]
pub struct SpawnOptions {
    #[arg(short, long, value_parser = clap::builder::PossibleValuesParser::new(["docker", "k8s", "native"]), default_value="docker")]
    provider: String,
    #[arg(
        short = 'd',
        long = "dir",
        help = "Directory path for placing the network files instead of random temp one"
    )]
    base_path: Option<String>,
    #[arg(
        short = 'c',
        long = "spawn-concurrency",
        help = "Number of concurrent spawning process to launch"
    )]
    spawn_concurrency: Option<usize>,
    /// Allow to manage how we verify node readiness or disable (None)
    /// For 'metric' we query prometheus 'process_start_time_seconds' in order to check the rediness".
    #[arg(
        short = 'v',
        long = "node-verifier",
        value_parser = clap::builder::PossibleValuesParser::new(["none", "metric"]), default_value="metric",
        verbatim_doc_comment,
    )]
    node_verifier: String,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    Spawn {
        /// Network config file path
        config: String,
        #[command(flatten)]
        spawn_opts: SpawnOptions,
    },
    Reproduce {
        /// Repository name (e.g. zombienet-sdk) - only needed if downloading from GitHub
        #[arg(required_unless_present = "archive_file")]
        repo: Option<String>,
        /// CI run id to reproduce - only needed if downloading from GitHub
        #[arg(required_unless_present = "archive_file")]
        run_id: Option<String>,
        #[arg(
            short = 'a',
            long = "archive",
            help = "Path to local nextest archive file (.tar.zst)",
            conflicts_with_all = ["repo", "run_id"]
        )]
        archive_file: Option<String>,
        #[arg(
            short = 't',
            long = "test",
            help = "Specific test to run (if not specified, will run all tests in archive)"
        )]
        test_filter: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let now = Instant::now();
    let args = Args::parse();

    match args.cmd {
        Commands::Spawn { config, spawn_opts } => {
            spawn_network(
                config,
                spawn_opts.provider,
                spawn_opts.base_path,
                spawn_opts.spawn_concurrency,
                spawn_opts.node_verifier,
                now,
            )
            .await
        },
        Commands::Reproduce {
            repo,
            run_id,
            archive_file,
            test_filter,
        } => reproduce(repo, run_id, archive_file, test_filter).await,
    }
}

async fn spawn_network(
    config: String,
    provider: String,
    base_path: Option<String>,
    spawn_concurrency: Option<usize>,
    node_verifier: String,
    start_time: Instant,
) -> Result<(), anyhow::Error> {
    let config = network_config(&config, base_path, spawn_concurrency);

    let provider: Provider = provider.into();
    let node_verifier: NodeVerifier = node_verifier.into();

    let spawn_fn = provider.get_spawn_fn();
    let network = spawn_fn(config).await.unwrap();

    if node_verifier == NodeVerifier::Metric {
        network
            .wait_until_is_up(20)
            .await
            .map_err(display_node_crash)?;
    }

    let elapsed = start_time.elapsed();
    println!("🚀🚀🚀 network is up, in {elapsed:.2?}");

    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        if node_verifier == NodeVerifier::Metric {
            network
                .wait_until_is_up(5)
                .await
                .map_err(display_node_crash)?;
        }
    }
}

async fn reproduce(
    repo: Option<String>,
    run_id: Option<String>,
    archive_file: Option<String>,
    test_filter: Option<String>,
) -> Result<(), anyhow::Error> {
    let archives = match archive_file {
        Some(path) => vec![validate_archive_path(&path)?],
        None => {
            let repo = repo.expect("repo is required when not using --archive");
            let run_id = run_id.expect("run_id is required when not using --archive");
            download_nextest_archives(&repo, &run_id, test_filter.as_deref())?
        },
    };

    run_all_nextest_archives(&archives)
}

fn validate_archive_path(path: &str) -> Result<String, anyhow::Error> {
    if !fs::metadata(path)?.is_file() {
        anyhow::bail!("Archive file does not exist: {}", path);
    }
    Ok(path.to_string())
}

fn download_nextest_archives(
    repo: &str,
    run_id: &str,
    test_filter: Option<&str>,
) -> Result<Vec<String>, anyhow::Error> {
    let temp_dir = std::path::PathBuf::from(format!("/tmp/zombienet-reproduce-{}", run_id));
    if !temp_dir.exists() {
        fs::create_dir_all(&temp_dir)?;
    }

    let pattern = match test_filter {
        Some(filter) => format!("*{}*", filter),
        None => "*zombienet-artifacts*".to_string(),
    };

    println!(
        "⬇️  Downloading nextest archive from GitHub run ID {} in repo paritytech/{}...",
        run_id, repo
    );
    if let Some(filter) = test_filter {
        println!("   Using filter pattern: *{}*", filter);
    }

    let output = std::process::Command::new("gh")
        .args([
            "run",
            "download",
            run_id,
            "--repo",
            &format!("paritytech/{}", repo),
            "--pattern",
            &pattern,
            "--dir",
        ])
        .arg(&temp_dir)
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute 'gh' command: {}\nInstall GitHub CLI and run: gh auth login",
                e
            )
        })?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to download artifacts.\n{}{}\nCheck GitHub CLI setup and permissions.",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    extract_and_find_archives(&temp_dir, run_id, repo)
}

fn extract_and_find_archives(
    dir: &std::path::Path,
    run_id: &str,
    repo: &str,
) -> Result<Vec<String>, anyhow::Error> {
    println!("📦 Extracting downloaded artifacts...");

    // First, extract all zip files
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("zip") {
            println!(
                "  Extracting: {}",
                path.file_name().unwrap().to_string_lossy()
            );
            let output = std::process::Command::new("unzip")
                .args(["-q", "-o"])
                .arg(&path)
                .arg("-d")
                .arg(dir)
                .output()?;

            if !output.status.success() {
                eprintln!(
                    "Warning: Failed to extract zip archive: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

    // Then, recursively search for .tar files and extract them
    extract_tar_files(dir)?;

    // Finally, find all .tar.zst files
    let archives = find_all_archives(dir)?;

    if archives.is_empty() {
        anyhow::bail!(
            "Could not find any nextest archives (.tar.zst) after extraction.\nVerify that run ID {} in paritytech/{} has nextest test archives.",
            run_id, repo
        );
    }

    println!("\n✓ Found {} nextest archive(s):", archives.len());
    for (i, archive) in archives.iter().enumerate() {
        let filename = std::path::Path::new(archive)
            .file_name()
            .unwrap()
            .to_string_lossy();
        println!("  {}. {}", i + 1, filename);
    }
    println!();

    Ok(archives)
}

fn run_all_nextest_archives(archives: &[String]) -> Result<(), anyhow::Error> {
    let workspace_path = get_workspace_path()?;

    for (i, archive_path) in archives.iter().enumerate() {
        println!("═══════════════════════════════════════════════════════════════");
        println!("Running archive {}/{}", i + 1, archives.len());
        println!("═══════════════════════════════════════════════════════════════\n");

        let archive_abs_path = std::path::PathBuf::from(archive_path)
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Failed to resolve archive path: {}", e))?;

        let archive_name = archive_abs_path.file_name().unwrap().to_string_lossy();

        println!("🚀 Running tests from: {}", archive_name);

        let inner_cmd = build_nextest_command();
        let mut cmd = build_docker_command(&archive_abs_path, &workspace_path, &inner_cmd);

        let status = cmd
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to execute docker command: {}\nMake sure Docker is installed and running.", e))?;

        if !status.success() {
            eprintln!(
                "\n❌ Tests from {} failed with exit code: {:?}\n",
                archive_name,
                status.code()
            );
        } else {
            println!("\n✅ Tests from {} completed successfully\n", archive_name);
        }
    }

    Ok(())
}

fn extract_tar_files(dir: &std::path::Path) -> Result<(), anyhow::Error> {
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("tar") {
            println!(
                "  Extracting tar: {}",
                path.file_name().unwrap().to_string_lossy()
            );
            let output = std::process::Command::new("tar")
                .args(["-xf"])
                .arg(&path)
                .arg("-C")
                .arg(dir)
                .output()?;

            if !output.status.success() {
                eprintln!(
                    "Warning: Failed to extract tar archive: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        } else if path.is_dir() {
            // Recursively extract tar files in subdirectories
            extract_tar_files(&path)?;
        }
    }
    Ok(())
}

fn find_all_archives(dir: &std::path::Path) -> Result<Vec<String>, anyhow::Error> {
    let mut archives = Vec::new();

    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                if ext == "zst" && path.to_string_lossy().ends_with(".tar.zst") {
                    archives.push(path.to_string_lossy().to_string());
                }
            }
        } else if path.is_dir() {
            // Recursively search subdirectories
            archives.extend(find_all_archives(&path)?);
        }
    }

    Ok(archives)
}

fn get_workspace_path() -> Result<String, anyhow::Error> {
    std::env::var("POLKADOT_SDK_PATH").map_err(|_| {
        anyhow::anyhow!(
            "POLKADOT_SDK_PATH environment variable is not set. \
            Please set it to the path of the polkadot-sdk workspace."
        )
    })
}

fn build_nextest_command() -> String {
    "export PATH=/workspace/target/release:$PATH && \
        cd /workspace && \
        cargo nextest run \
        --archive-file /archive.tar.zst \
        --workspace-remap /workspace \
        --no-capture; \
        echo ''; \
        echo '=== Tests completed ==='"
        .to_string()
}

fn build_docker_command(
    archive_path: &std::path::Path,
    workspace_path: &str,
    inner_cmd: &str,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("docker");
    cmd.args([
        "run",
        "-it",
        "--rm",
        "-v",
        &format!("{}:/archive.tar.zst:ro", archive_path.display()),
        "-v",
        &format!("{}:/workspace", workspace_path),
        "-e",
        "ZOMBIE_PROVIDER=native",
        "-e",
        "RUST_LOG=info",
        "docker.io/paritytech/ci-unified:bullseye-1.88.0-2025-06-27-v202506301118",
        "bash",
        "-c",
        inner_cmd,
    ]);
    cmd
}

fn display_node_crash(e: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!("\n\t🧟 One of the nodes crashed, {}", e.to_string())
}

pub fn network_config(
    config: &str,
    base_path: Option<String>,
    concurrency: Option<usize>,
) -> NetworkConfig {
    let network_config = NetworkConfig::load_from_toml(config).unwrap();
    let tear_down_on_failure = network_config.global_settings().tear_down_on_failure();

    // nothing to override
    if base_path.is_none() && concurrency.is_none() && !tear_down_on_failure {
        return network_config;
    }

    let current_settings = network_config.global_settings();
    let bootnodes_addresses: Vec<String> = current_settings
        .bootnodes_addresses()
        .iter()
        .map(|x| x.to_string())
        .collect();

    let settings_builder = GlobalSettingsBuilder::new()
        .with_bootnodes_addresses(bootnodes_addresses.iter().map(|x| x.as_str()).collect())
        .with_network_spawn_timeout(current_settings.network_spawn_timeout())
        .with_node_spawn_timeout(current_settings.node_spawn_timeout())
        .with_tear_down_on_failure(false);

    let settings_builder = if let Some(local_ip) = current_settings.local_ip() {
        settings_builder.with_local_ip(local_ip.to_string().as_str())
    } else {
        settings_builder
    };

    // overrides if needed
    let settings_builder = if let Some(base_path) = base_path {
        settings_builder.with_base_dir(base_path)
    } else {
        // check if is already defined
        if let Some(base_path) = current_settings.base_dir() {
            settings_builder.with_base_dir(base_path)
        } else {
            settings_builder
        }
    };

    let settings_builder = if let Some(concurrency) = concurrency {
        settings_builder.with_spawn_concurrency(concurrency)
    } else {
        // check if is already defined
        if let Some(concurrency) = current_settings.spawn_concurrency() {
            settings_builder.with_spawn_concurrency(concurrency)
        } else {
            settings_builder
        }
    };

    let settings = settings_builder.build().unwrap();
    NetworkConfig::load_from_toml_with_settings(config, &settings).unwrap()
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn works_without_any() {
        let n = network_config("./testing/config.toml", None, None);
        assert_eq!(
            n.global_settings().base_dir(),
            Some(PathBuf::from("/tmp/zombie-bite_1751850079747/spawn").as_path())
        );
        assert_eq!(n.global_settings().spawn_concurrency(), Some(4));
    }

    #[test]
    fn works_with_base_path() {
        let overrided = String::from("/tmp/overrided");
        let expected = PathBuf::from("/tmp/overrided");
        let n = network_config("./testing/config.toml", Some(overrided), None);
        assert_eq!(n.global_settings().base_dir(), Some(expected.as_path()));
        assert_eq!(n.global_settings().spawn_concurrency(), Some(4));
    }

    #[test]
    fn works_with_concurrency() {
        let n = network_config("./testing/config.toml", None, Some(1));
        assert_eq!(
            n.global_settings().base_dir(),
            Some(PathBuf::from("/tmp/zombie-bite_1751850079747/spawn").as_path())
        );
        assert_eq!(n.global_settings().spawn_concurrency(), Some(1));
    }

    #[test]
    fn works_with_both() {
        let overrided = String::from("/tmp/overrided");
        let expected = PathBuf::from("/tmp/overrided");
        let n = network_config("./testing/config.toml", Some(overrided), Some(1));
        assert_eq!(n.global_settings().base_dir(), Some(expected.as_path()));
        assert_eq!(n.global_settings().spawn_concurrency(), Some(1));
        assert!(!n.global_settings().tear_down_on_failure())
    }
}
