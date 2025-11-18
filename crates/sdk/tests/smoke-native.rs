use std::time::Instant;

use configuration::{NetworkConfig, NetworkConfigBuilder};
use zombienet_sdk::environment::get_spawn_fn;

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.7.0")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(3000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator-omni")
                    .with_command("polkadot-omni-node")
            })
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
    println!("🚀🚀🚀🚀 network deployed in {elapsed:.2?}");

    network.wait_until_is_up(20).await.unwrap();

    let elapsed = now.elapsed();
    println!("✅✅✅✅ network is up in {elapsed:.2?}");

    // Get a ref to the node
    let alice = network.get_node("alice").unwrap();
    // wait 10 blocks
    alice
        .wait_metric(BEST_BLOCK_METRIC, |x| x > 9_f64)
        .await
        .unwrap();
}
