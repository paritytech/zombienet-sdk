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
        .with_parachain(|p| {
            p.with_id(2000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator")
                    .with_command("polkadot-parachain")
                    .with_image("docker.io/parity/polkadot-parachain:1.7.0")
            })
        })
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_k8s_basic_functionalities_should_works() {
    tracing_subscriber::fmt::init();
    let now = Instant::now();

    let config = small_network();
    let spawn_fn = get_spawn_fn();

    let network = spawn_fn(config).await.unwrap();

    let elapsed = now.elapsed();
    println!("🚀🚀🚀🚀 network deployed in {elapsed:.2?}");

    // Get a ref to the node
    let alice = network.get_node("alice").unwrap();

    alice.wait_until_is_up(90_u64).await.unwrap();
}
