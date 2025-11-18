use configuration::NetworkConfig;
use zombienet_sdk::environment::get_spawn_fn;

const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
const CONFIG_PATH: &str = "tests/configs/block-building.toml";
const INTEGRATION_IMAGE_ENV: &str = "ZOMBIENET_INTEGRATION_TEST_IMAGE";
const DEFAULT_SUBSTRATE_IMAGE: &str = "docker.io/paritypr/substrate:master";

fn ensure_integration_test_image() {
    if std::env::var(INTEGRATION_IMAGE_ENV).is_err() {
        std::env::set_var(INTEGRATION_IMAGE_ENV, DEFAULT_SUBSTRATE_IMAGE);
    }
}

fn config_file() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{manifest_dir}/{CONFIG_PATH}")
}

#[tokio::test(flavor = "multi_thread")]
async fn block_building_local_chain_produces_blocks() {
    tracing_subscriber::fmt::init();

    ensure_integration_test_image();
    let config = NetworkConfig::load_from_toml("./crates/examples/examples/0003-arg-removal.toml").unwrap();
    let spawn_fn = get_spawn_fn();

    let network = spawn_fn(config).await.unwrap();

    let alice = network.get_node("alice")?;
    alice
        .wait_metric(BEST_BLOCK_METRIC, |x| x > 2_f64)
        .await
        .unwrap();

    let bob = network.get_node("bob").unwrap();
    bob.wait_metric(BEST_BLOCK_METRIC, |x| x > 2_f64)
        .await
        .unwrap();
}

