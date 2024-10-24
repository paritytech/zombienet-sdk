// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

use std::env;

use anyhow::anyhow;
use zombienet_sdk::{
    tx_helper::{ChainUpgrade, RuntimeUpgradeOptions},
    NetworkConfigBuilder,
};

const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();

    // allow to pass the upgrade path through first cli argument
    let args: Vec<_> = env::args().collect();

    let images = zombienet_sdk::environment::get_images_from_env();
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image(images.polkadot.as_str())
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .with_default_command("test-parachain")
                .with_default_image(images.cumulus.as_str())
                .with_collator(|c| c.with_name("collator"))
        })
        .build()
        .map_err(|e| {
            let errs = e
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            anyhow!("config errs: {errs}")
        })?;

    let spawn_fn = zombienet_sdk::environment::get_spawn_fn();
    let network = spawn_fn(config).await?;

    // wait 2 blocks
    let alice = network.get_node("alice")?;
    assert!(alice
        .wait_metric(BEST_BLOCK_METRIC, |b| b > 2_f64)
        .await
        .is_ok());

    // get current runtime spec
    let client = network
        .get_node("collator")?
        .client::<subxt::PolkadotConfig>()
        .await?;
    let current_runtime = client.backend().current_runtime_version().await?;
    println!(
        "current_runtime spec version: {:?}",
        current_runtime.spec_version
    );

    // get current best
    let best_block = alice.reports(BEST_BLOCK_METRIC).await?;

    // upgrade runtime
    let wasm = if args.len() > 1 {
        args[1].clone()
    } else if env::var("ZOMBIE_WASM_INCREMENTED_PATH").is_ok() {
        env::var("ZOMBIE_WASM_INCREMENTED_PATH").unwrap()
    } else {
        panic!("You need to provide the PATH to the wasm file to use to upgrade, through first argument or 'ZOMBIE_WASM_INCREMENTED_PATH' env var");
    };

    println!("Perfoming upgrade from path {wasm}");

    network
        .parachain(100)
        .expect("Invalid parachain Id")
        .runtime_upgrade(RuntimeUpgradeOptions::new(wasm.as_str().into()))
        .await?;

    // wait 2 more blocks
    alice
        .wait_metric(BEST_BLOCK_METRIC, |x| x > best_block + 2_f64)
        .await?;

    let incremented_runtime = client.backend().current_runtime_version().await?;
    println!(
        "incremented_runtime spec version: {}",
        incremented_runtime.spec_version
    );

    assert_eq!(
        incremented_runtime.spec_version,
        current_runtime.spec_version + 1000,
        "version should be incremented"
    );

    Ok(())
}
