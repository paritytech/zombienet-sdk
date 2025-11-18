use configuration::NetworkConfig;
use orchestrator::network::node::{LogLineCountOptions, NetworkNode};
use std::time::Duration;
use subxt::{dynamic::tx, PolkadotConfig};
use subxt_signer::sr25519::dev;

const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
const CONFIG_PATH: &str = "tests/configs/block-building.toml";
const INTEGRATION_IMAGE_ENV: &str = "ZOMBIENET_INTEGRATION_TEST_IMAGE";
const DEFAULT_SUBSTRATE_IMAGE: &str = "docker.io/paritypr/substrate:latest";
const NODE_ROLE_METRIC: &str = "node_roles";
const PEER_COUNT_METRIC: &str = "substrate_sub_libp2p_peers_count";
const ROLE_VALIDATOR_VALUE: f64 = 4.0;
const PEER_MIN_THRESHOLD: f64 = 1.0;
const BLOCK_TARGET: f64 = 5.0;
const BLOCK_TIMEOUT_SECS: u64 = 20;
const LOG_TIMEOUT_SECS: u64 = 2;
const SCRIPT_TIMEOUT_SECS: u64 = 30;

fn config_file() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{manifest_dir}/{CONFIG_PATH}")
}

async fn assert_node_health(node: &NetworkNode) {
    node.wait_metric_with_timeout(
        NODE_ROLE_METRIC,
        |role| (role - ROLE_VALIDATOR_VALUE).abs() < f64::EPSILON,
        BLOCK_TIMEOUT_SECS,
    )
    .await
    .unwrap();

    node.wait_metric_with_timeout(
        PEER_COUNT_METRIC,
        |peers| peers >= PEER_MIN_THRESHOLD,
        BLOCK_TIMEOUT_SECS,
    )
    .await
    .unwrap();

    node.wait_metric_with_timeout(
        BEST_BLOCK_METRIC,
        |height| height >= BLOCK_TARGET,
        BLOCK_TIMEOUT_SECS,
    )
    .await
    .unwrap();

    node.wait_log_line_count_with_timeout(
        "error",
        false,
        LogLineCountOptions::no_occurences_within_timeout(Duration::from_secs(LOG_TIMEOUT_SECS)),
    )
    .await
    .unwrap();
}

async fn submit_transaction_and_wait_finalization(node: &NetworkNode) {
    let client = node.wait_client::<PolkadotConfig>().await.unwrap();

    let alice_signer = dev::alice();

    tokio::time::timeout(Duration::from_secs(SCRIPT_TIMEOUT_SECS), async {
        let remark_call = tx(
            "System",
            "remark",
            vec![subxt::dynamic::Value::from_bytes(b"block-building-test")],
        );
        client
            .tx()
            .sign_and_submit_then_watch_default(&remark_call, &alice_signer)
            .await
            .expect("submit transfer")
            .wait_for_finalized_success()
            .await
            .expect("transaction finalized");
    })
    .await
    .expect("transaction completes within timeout");
}

#[tokio::test(flavor = "multi_thread")]
async fn block_building_local_chain_produces_blocks() {
    let _ = tracing_subscriber::fmt::try_init();
    if std::env::var(INTEGRATION_IMAGE_ENV).is_err() {
        std::env::set_var(INTEGRATION_IMAGE_ENV, DEFAULT_SUBSTRATE_IMAGE);
    }
    let config = NetworkConfig::load_from_toml(&config_file()).unwrap();
    let spawn_fn = zombienet_sdk::environment::get_spawn_fn();
    let network = spawn_fn(config).await.unwrap();

    let alice = network.get_node("alice").unwrap();
    let bob = network.get_node("bob").unwrap();

    assert_node_health(&alice).await;
    assert_node_health(&bob).await;

    submit_transaction_and_wait_finalization(&alice).await;
}
