use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use tempfile::Builder;
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let temp_dir = Builder::new()
        .prefix("zombienet-chain-spec-")
        .tempdir()
        .context("creating temporary directory")?;

    let temp_path = temp_dir.path().to_path_buf();

    let kusama_runtime_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../orchestrator/testing/kusama_runtime-v2000002.compact.compressed.wasm");
    let asset_hub_runtime_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../orchestrator/testing/asset-hub-kusama_runtime-v2000002.compact.compressed.wasm");

    let config = NetworkConfigBuilder::new()
        .with_relaychain(|relaychain| {
            relaychain
                .with_chain("kusama-local")
                .with_default_image("docker.io/parity/polkadot:latest")
                .with_default_command("polkadot")
                .with_chain_spec_runtime(kusama_runtime_path, Some("local_testnet"))
                .with_validator(|node| node.with_name("alice"))
        })
        .with_parachain(|parachain| {
            parachain
                .with_id(1000)
                .with_chain("asset-hub-kusama-local")
                .with_default_image("docker.io/parity/polkadot-parachain:latest")
                .with_default_command("polkadot-parachain")
                .with_chain_spec_runtime(asset_hub_runtime_path, Some("local_testnet"))
                .with_collator(|collator| collator.with_name("asset-hub-collator-1"))
        })
        .with_global_settings(|settings| settings.with_base_dir(temp_path))
        .build()
        .map_err(|errors| {
            let message = errors
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            anyhow!("failed to build network configuration: {message}")
        })?;

    let base_dir_from_settings = config.global_settings().base_dir().map(|p| p.to_path_buf());

    let network = config.spawn_docker().await?;

    println!("🚀🚀🚀🚀 network deployed");

    let base_dir = network
        .base_dir()
        .map(PathBuf::from)
        .or(base_dir_from_settings)
        .ok_or_else(|| anyhow!("base directory not available from network or config"))?;

    let cases = [
        ("kusama-local", Some("/genesis/raw/top")),
        ("asset-hub-kusama-local", Some("/genesis/raw/top")),
    ];

    let mut results = Map::new();
    for (prefix, raw_pointer) in cases {
        let plain_path = base_dir.join(format!("{prefix}-plain.json"));
        anyhow::ensure!(
            plain_path.exists(),
            "plain chain-spec was not generated at {}",
            plain_path.display()
        );

        let raw_path = base_dir.join(format!("{prefix}.json"));
        anyhow::ensure!(
            raw_path.exists(),
            "raw chain-spec was not generated at {}",
            raw_path.display()
        );

        let raw_json: Value = serde_json::from_str(
            &std::fs::read_to_string(&raw_path)
                .with_context(|| format!("reading {}", raw_path.display()))?,
        )
            .with_context(|| format!("parsing {}", raw_path.display()))?;

        if let Some(pointer) = raw_pointer {
            anyhow::ensure!(
                raw_json.pointer(pointer).is_some(),
                "raw chain-spec '{prefix}' missing '{pointer}' section"
            );
        }

        results.insert(
            prefix.to_string(),
            Value::String(format!(
                "plain: {}, raw: {}",
                plain_path.display(),
                raw_path.display()
            )),
        );
    }

    println!(
        "Generated chain-specs:\n{}",
        serde_json::to_string_pretty(&Value::Object(results))?
    );

    network.destroy().await?;

    Ok(())
}