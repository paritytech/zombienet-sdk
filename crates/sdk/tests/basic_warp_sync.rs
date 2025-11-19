use std::time::Duration;

use configuration::NetworkConfig;
use orchestrator::network::node::LogLineCountOptions;

const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
const CONFIG_PATH: &str = "tests/configs/test-warp-sync.toml";
const INTEGRATION_IMAGE_ENV: &str = "ZOMBIENET_INTEGRATION_TEST_IMAGE";
const DB_SNAPSHOT_ENV: &str = "DB_SNAPSHOT";
const CHAIN_SPEC_ENV: &str = "WARP_CHAIN_SPEC_PATH";
const DB_BLOCK_HEIGHT_ENV: &str = "DB_BLOCK_HEIGHT";
const DEFAULT_SUBSTRATE_IMAGE: &str = "docker.io/paritypr/substrate:latest";
const DEFAULT_DB_SNAPSHOT_URL: &str = "https://storage.googleapis.com/zombienet-db-snaps/substrate/0001-basic-warp-sync/chains-0bb3f0be2ce41b5615b224215bcc8363aa0416a6.tgz";
const DEFAULT_CHAIN_SPEC: &str = "https://raw.githubusercontent.com/paritytech/polkadot-sdk/refs/heads/master/substrate/zombienet/0001-basic-warp-sync/chain-spec.json";
const ROLE_TIMEOUT_SECS: u64 = 60;
const PEER_TIMEOUT_SECS: u64 = 60;
const BOOTSTRAP_TIMEOUT_SECS: u64 = 180;
const METRIC_TIMEOUT_SECS: u64 = 60;
const LOG_TIMEOUT_LONG_SECS: u64 = 60;
const LOG_TIMEOUT_SHORT_SECS: u64 = 10;
const LOG_ERROR_TIMEOUT_SECS: u64 = 10;
const PEERS_THRESHOLD: f64 = 3.0;
const MIN_BOOTSTRAP_BLOCK: f64 = 1.0;

fn config_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{manifest_dir}/{CONFIG_PATH}")
}

fn ensure_env_defaults() {
    if std::env::var(INTEGRATION_IMAGE_ENV).is_err() {
        std::env::set_var(INTEGRATION_IMAGE_ENV, DEFAULT_SUBSTRATE_IMAGE);
    }

    if std::env::var(DB_SNAPSHOT_ENV).is_err() {
        std::env::set_var(DB_SNAPSHOT_ENV, DEFAULT_DB_SNAPSHOT_URL);
    }

    if std::env::var(CHAIN_SPEC_ENV).is_err() {
        std::env::set_var(CHAIN_SPEC_ENV, DEFAULT_CHAIN_SPEC);
    }
}

fn db_snapshot_height_override() -> Option<f64> {
    std::env::var(DB_BLOCK_HEIGHT_ENV)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
}

#[tokio::test(flavor = "multi_thread")]
async fn basic_warp_sync() {
    let _ = tracing_subscriber::fmt::try_init();
    ensure_env_defaults();

    let config = NetworkConfig::load_from_toml(&config_path()).unwrap();
    let spawn_fn = zombienet_sdk::environment::get_spawn_fn();
    let network = spawn_fn(config).await.unwrap();

    network
        .wait_until_is_up(BOOTSTRAP_TIMEOUT_SECS)
        .await
        .expect("network becomes ready");

    println!("🚀🚀🚀🚀 network deployed");

    let alice = network.get_node("alice").unwrap();
    assert!(alice
        .wait_metric(BEST_BLOCK_METRIC, |b| b > 2_f64)
        .await
        .is_ok());

    let dave = network.get_node("dave").unwrap();
    let logs = dave.logs().await.unwrap();
    println!("dave logs:\n{logs}");

    // Role + peer-count checks for all nodes.
    for node_name in ["alice", "bob", "charlie", "dave"] {
        let node = network.get_node(node_name).unwrap();
        node.wait_metric_with_timeout(
            "node_roles",
            |role| (role - 1.0).abs() < f64::EPSILON,
            ROLE_TIMEOUT_SECS,
        )
        .await
        .unwrap();

        node.wait_metric_with_timeout(
            "substrate_sub_libp2p_peers_count",
            |peers| peers >= PEERS_THRESHOLD,
            PEER_TIMEOUT_SECS,
        )
        .await
        .unwrap();
    }

    for node_name in ["alice", "bob", "charlie"] {
        let node = network.get_node(node_name).unwrap();
        node.wait_metric_with_timeout(
            BEST_BLOCK_METRIC,
            |x| x >= MIN_BOOTSTRAP_BLOCK,
            BOOTSTRAP_TIMEOUT_SECS,
        )
        .await
        .unwrap();
    }

    let db_snapshot_height = match db_snapshot_height_override() {
        Some(value) => value,
        None => network
            .get_node("alice")
            .unwrap()
            .reports(BEST_BLOCK_METRIC)
            .await
            .unwrap(),
    };

    for node_name in ["alice", "bob", "charlie"] {
        network
            .get_node(node_name)
            .unwrap()
            .wait_metric_with_timeout(
                BEST_BLOCK_METRIC,
                |x| x >= db_snapshot_height,
                METRIC_TIMEOUT_SECS,
            )
            .await
            .unwrap();
    }

    // Dave runs with warp-sync arguments and should follow finalized heads quickly
    let dave = network.get_node("dave").unwrap();
    dave.wait_metric_with_timeout(BEST_BLOCK_METRIC, |x| x >= 1_f64, METRIC_TIMEOUT_SECS)
        .await
        .unwrap();
    dave.wait_metric_with_timeout(
        BEST_BLOCK_METRIC,
        |x| x >= db_snapshot_height,
        METRIC_TIMEOUT_SECS,
    )
    .await
    .unwrap();

    // Logs indicating successful warp/state sync
    let at_least_once = |timeout_secs| {
        LogLineCountOptions::new(|count| count >= 1, Duration::from_secs(timeout_secs), false)
    };
    dave.wait_log_line_count_with_timeout(
        "Warp sync is complete",
        false,
        at_least_once(LOG_TIMEOUT_LONG_SECS),
    )
    .await
    .unwrap();
    dave.wait_log_line_count_with_timeout(
        r"Checking for displaced leaves after finalization\. leaves=\[0xc5e7b4cfd23932bb930e859865430a35f6741b4732d677822d492ca64cc8d059\]",
        false,
        at_least_once(LOG_TIMEOUT_SHORT_SECS),
    )
    .await
    .unwrap();
    dave.wait_log_line_count_with_timeout(
        "State sync is complete",
        false,
        at_least_once(LOG_TIMEOUT_LONG_SECS),
    )
    .await
    .unwrap();
    dave.wait_log_line_count_with_timeout(
        "Block history download is complete",
        false,
        at_least_once(LOG_TIMEOUT_SHORT_SECS),
    )
    .await
    .unwrap();

    dave.wait_log_line_count_with_timeout(
        "error",
        false,
        LogLineCountOptions::no_occurences_within_timeout(Duration::from_secs(
            LOG_ERROR_TIMEOUT_SECS,
        )),
    )
    .await
    .unwrap();
    dave.wait_log_line_count_with_timeout(
        "verification failed",
        false,
        LogLineCountOptions::no_occurences_within_timeout(Duration::from_secs(
            LOG_ERROR_TIMEOUT_SECS,
        )),
    )
    .await
    .unwrap();

    network.destroy().await.unwrap();
}
