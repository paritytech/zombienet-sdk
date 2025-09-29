use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    Spawn {
        config: String,
        #[arg(short, long, value_parser = clap::builder::PossibleValuesParser::new(["docker", "k8s", "native"]), default_value="docker")]
        provider: String,
        #[arg(
            short = 'd',
            long = "dir",
            help = "Directory path for placing the network files instead of random temp one (e.g. -d /home/user/my-zombienet)"
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

    let (config, provider, base_path, spawn_concurrency, node_verifier) = match args.cmd {
        Commands::Spawn {
            config,
            provider,
            base_path,
            spawn_concurrency,
            node_verifier,
        } => (
            config,
            provider,
            base_path,
            spawn_concurrency,
            node_verifier,
        ),
    };

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

    let elapsed = now.elapsed();
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
