use std::time::Instant;

use configuration::{NetworkConfig, NetworkConfigBuilder};
use zombienet_sdk::environment::get_spawn_fn;

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.7.0")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_native_smoke_should_works() {
    tracing_subscriber::fmt::init();
    const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
    let now = Instant::now();
    let config = small_network();
    let spawn_fn = get_spawn_fn();

    let network = spawn_fn(config).await.unwrap();

    let elapsed = now.elapsed();
    println!("🚀🚀🚀🚀 network deployed in {:.2?}", elapsed);

    // Get a ref to the node
    let alice = network.get_node("alice").unwrap();
    // wait 10 blocks
    alice
        .wait_metric(BEST_BLOCK_METRIC, |x| x > 9_f64)
        .await
        .unwrap();
}
