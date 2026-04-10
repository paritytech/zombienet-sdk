use std::time::Instant;

use configuration::{NetworkConfig, NetworkConfigBuilder};
use zombienet_sdk::environment::get_spawn_fn;

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node_group(|ng| {
                    ng.with_base_node(|n| n.with_name("validator"))
                        .with_count(6)
                })
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_num_cores(3)
                .with_collator(|n| n.with_name("collator").with_command("test-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(3000)
            // .with_num_cores(3)
                .with_collator(|n| n.with_name("collator-test").with_command("test-parachain"))
        })
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_native_smoke_core_assign() {
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
    let validator = network.get_node("validator-0").unwrap();
    // wait 15 blocks
    validator
        .wait_metric(BEST_BLOCK_METRIC, |x| x > 15_f64)
        .await
        .unwrap();
}
