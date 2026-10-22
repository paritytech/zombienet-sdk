//! A JAM network whose genesis, built in process, carries a service of the test's own: the
//! chain must start, and the service must be in its state.
//!
//! Needs `polkajam` in `PATH`.

use std::time::{Duration, Instant};

use anyhow::Context;
use configuration::{NetworkConfig, NetworkConfigBuilder};
use serde_json::Value;
use subxt::ext::jsonrpsee::rpc_params;
use zombienet_sdk::{
    environment::get_spawn_fn,
    jam::{JamAuthorizer, JamBalance, BOOTSTRAP_SERVICE_BLOB},
    subxt::ext::jsonrpsee::core::client::ClientT,
    JamNetworkNode,
};

pub const DEADLINE: Duration = Duration::from_secs(5 * 60);

const SERVICE_ID: u32 = 5;

/// Slots in a `tiny` epoch. The first epoch's sealing keys come from the genesis zombienet
/// builds, so the chain has to finalize past it.
const EPOCH_PERIOD: u64 = 12;

fn network_with_service(service_code: &str) -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_jamchain(|jamchain| {
            let mut jamchain = jamchain
                .with_id("dev")
                .with_genesis(|g| {
                    g.with_service(SERVICE_ID, |s| {
                        s.with_code(service_code)
                            .with_balance(JamBalance::MinimumPlus(1_000_000))
                            .with_storage(b"greeting".to_vec(), b"hello".to_vec())
                    })
                    .with_auth_queue(1, [JamAuthorizer::Null])
                    .with_assigner([1], SERVICE_ID)
                })
                .with_validator(|n| n.with_name("jam0"));
            for i in 1..6 {
                jamchain = jamchain.with_validator(|n| n.with_name(&format!("jam{i}")));
            }
            jamchain.with_ordinary(|n| n.with_name("jam-or"))
        })
        .build()
        .unwrap()
}

/// Wait until the node reports a synced chain that finalized `slots` slots past the first
/// finalized block it reports, and return the last finalized block.
async fn wait_finalized_for(
    node: &JamNetworkNode,
    slots: u64,
    deadline: Instant,
) -> anyhow::Result<Value> {
    let client = node.wait_client_with_timeout(60_u64).await?;
    let mut first_slot = None;
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
            let first_slot = *first_slot.get_or_insert(slot);
            if slot >= first_slot + slots {
                return Ok(block);
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    Err(anyhow::anyhow!(
        "[{}] did not finalize {slots} slots in time (first finalized slot {first_slot:?})",
        node.name()
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_native_smoke_with_genesis_service_should_works() {
    tracing_subscriber::fmt::init();

    let dir = tempfile::tempdir().unwrap();
    let service_code = dir.path().join("service.jam");
    std::fs::write(&service_code, BOOTSTRAP_SERVICE_BLOB).unwrap();

    let network = get_spawn_fn()(network_with_service(&service_code.to_string_lossy()))
        .await
        .unwrap();

    let rpc_node = network
        .get_jam_node("jam-or")
        .expect("jam-or should be present");
    let block = wait_finalized_for(rpc_node, EPOCH_PERIOD + 1, Instant::now() + DEADLINE)
        .await
        .expect("the chain finalizes");

    let client = rpc_node.wait_client_with_timeout(60_u64).await.unwrap();
    let service: Value = client
        .request(
            "serviceData",
            rpc_params![block["header_hash"].clone(), SERVICE_ID],
        )
        .await
        .unwrap();
    assert!(
        !service.is_null(),
        "service {SERVICE_ID} is not in the state"
    );
    let missing: Value = client
        .request(
            "serviceData",
            rpc_params![block["header_hash"].clone(), SERVICE_ID + 1],
        )
        .await
        .unwrap();
    assert!(missing.is_null());
}
