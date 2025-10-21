use std::{
    fs,
    path::Path,
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
        /// Repository name (e.g. polkadot, cumulus, substrate)
        repo: String,
        /// CI job id to reproduce
        job_id: String,
        #[arg(
            short = 'n',
            long = "network",
            help = "Path to network config file (if not specified, will download from GitHub artifacts)"
        )]
        network_file: Option<String>,
        #[command(flatten)]
        spawn_opts: SpawnOptions,
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
            job_id,
            network_file,
            spawn_opts,
        } => reproduce(repo, job_id, network_file, spawn_opts, now).await,
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
    repo: String,
    job_id: String,
    network_file: Option<String>,
    spawn_opt: SpawnOptions,
    start_time: Instant,
) -> Result<(), anyhow::Error> {
    println!("\nℹ️  Reproduce - reproducing CI job locally");
    println!("   Repository: paritytech/{}", repo);
    println!("   Job ID: {}\n", job_id);

    let network_config_path = match network_file {
        Some(path) => validate_network_file(path)?,
        None => download_and_extract_network_config(&repo, &job_id)?,
    };

    println!("✓ Found network config: {}", network_config_path);
    println!(
        "  Spawning network with {} provider...\n",
        spawn_opt.provider
    );

    spawn_network(
        network_config_path,
        spawn_opt.provider,
        spawn_opt.base_path,
        spawn_opt.spawn_concurrency,
        spawn_opt.node_verifier,
        start_time,
    )
    .await
}

fn validate_network_file(path: String) -> Result<String, anyhow::Error> {
    if !Path::new(&path).exists() {
        anyhow::bail!("Network file does not exist: {}", path);
    }
    println!("✓ Using provided network file: {}\n", path);
    Ok(path)
}

fn download_and_extract_network_config(repo: &str, job_id: &str) -> Result<String, anyhow::Error> {
    let artifacts_dir = prepare_artifacts_directory(job_id)?;
    let artifacts_path = artifacts_dir.to_string_lossy().to_string();

    println!("⬇️  Downloading artifacts using GitHub CLI...");
    download_artifacts_with_gh(repo, job_id, &artifacts_path)?;
    println!("✓ Artifacts downloaded to: {}\n", artifacts_path);

    find_zombie_json(&artifacts_path, repo, job_id)
}

fn prepare_artifacts_directory(job_id: &str) -> Result<std::path::PathBuf, anyhow::Error> {
    let temp_dir = std::env::temp_dir().join(format!("zombienet-reproduce-{}", job_id));

    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }

    fs::create_dir_all(&temp_dir)?;
    Ok(temp_dir)
}

fn find_zombie_json(
    artifacts_path: &str,
    repo: &str,
    job_id: &str,
) -> Result<String, anyhow::Error> {
    fs::read_dir(artifacts_path)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("zombie.json"))
        })
        .map(|path| path.to_string_lossy().to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find 'zombie.json' in artifacts at: {}\n\
                Verify that run ID {} in paritytech/{} has zombienet artifacts.\n\
                Or specify a network file directly: zombie-cli reproduce {} {} -n /path/to/network.toml",
                artifacts_path, job_id, repo, repo, job_id
            )
        })
}

fn download_artifacts_with_gh(
    repo: &str,
    run_id: &str,
    output_dir: &str,
) -> Result<(), anyhow::Error> {
    let output = std::process::Command::new("gh")
        .arg("run")
        .arg("download")
        .arg(run_id)
        .arg("--repo")
        .arg(format!("paritytech/{}", repo))
        .arg("--dir")
        .arg(output_dir)
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute 'gh' command: {}\nInstall GitHub CLI and run: gh auth login",
                e
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Failed to download artifacts.\n{}{}\nCheck GitHub CLI setup and permissions.",
            stdout,
            stderr
        );
    }

    Ok(())
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
