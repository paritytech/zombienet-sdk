use std::{
    println,
    time::{Duration, Instant},
};

use anyhow::Context;
use configuration::{NetworkConfig, NetworkConfigBuilder};
use serde_json::Value;
use subxt::ext::jsonrpsee::rpc_params;
use zombienet_sdk::{
    environment::get_spawn_fn, subxt::ext::jsonrpsee::core::client::ClientT, JamNetworkNode,
};

pub const DEADLINE: Duration = Duration::from_secs(5 * 60);

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_tiny_jamchain()
        .build()
        .unwrap()
}

/// Connect, then wait until the node reports a synced chain that has moved past genesis.
///
/// That is exactly `jamt`'s own precondition (`NodeExt::wait_for_sync`), so getting here first
/// means the later `jamt create-service` cannot silently block forever on a chain that never
/// started.
async fn wait_ready(node: &JamNetworkNode, deadline: Instant) -> anyhow::Result<()> {
    let client = node.wait_client_with_timeout(60_u64).await?;
    while Instant::now() < deadline {
        let sync_state: Value = client
            .request("syncState", rpc_params![])
            .await
            .context("syncState")?;
        let block: Value = client
            .request("finalizedBlock", rpc_params![])
            .await
            .context("finalizedBlock")?;
        let slot = block["slot"].as_u64().unwrap_or(0);

        if sync_state["status"] == "Completed" && slot > 0 {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let state: Value = client
        .request("syncState", rpc_params![])
        .await
        .context("syncState")?;
    Err(anyhow::anyhow!(
        "[{}] did not finalize a block in time (syncState: {state:?})",
        node.name()
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_native_smoke_should_works() {
    tracing_subscriber::fmt::init();

    let now = Instant::now();
    let config = small_network();
    let spawn_fn = get_spawn_fn();

    let network = spawn_fn(config).await.unwrap();

    let elapsed = now.elapsed();
    println!("🚀🚀🚀🚀 network deployed in {elapsed:.2?}");

    let deadline = Instant::now() + DEADLINE;
    let rpc_node = network
        .get_jam_node("jam-or")
        .expect("jam-or should be present");
    wait_ready(rpc_node, deadline)
        .await
        .expect("wait_ready state");
}
