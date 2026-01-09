use std::{env, path::PathBuf};

use configuration::{NetworkConfig, NetworkConfigBuilder};
use subxt::{ext::futures::StreamExt, OnlineClient, PolkadotConfig};
use zombienet_sdk::NetworkConfigExt;

fn small_network() -> NetworkConfig {
    let relay_runtime_path = PathBuf::from(env::var("RELAY_RUNTIME_PATH").unwrap());
    let polkadot_bin_latest = env::var("POLKADOT_BIN_LATEST").unwrap_or("polkadot".into());
    let polkadot_bin_latest_1 = env::var("POLKADOT_BIN_LATEST-1").unwrap_or("polkadot".into());
    let polkadot_bin_latest_2 = env::var("POLKADOT_BIN_LATEST-2").unwrap_or("polkadot".into());

    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("polkadot-local")
                .with_default_command("polkadot")
                .with_default_args(vec!["-lparachain=debug,runtime=debug".into()])
                .with_chain_spec_runtime(relay_runtime_path, None)
                .with_validator(|node| {
                    node.with_name("alice")
                        .with_command(polkadot_bin_latest.as_ref())
                })
                .with_validator(|node| {
                    node.with_name("bob")
                        .with_command(polkadot_bin_latest_1.as_ref())
                })
                .with_validator(|node| {
                    node.with_name("charlie")
                        .with_command(polkadot_bin_latest_2.as_ref())
                })
        })
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_native_smoke_should_works() {
    tracing_subscriber::fmt::init();
    let config = small_network();
    let network = config.spawn_native().await.unwrap();

    let alice = network.get_node("alice").unwrap();
    let alice_client: OnlineClient<PolkadotConfig> = alice.wait_client().await.unwrap();

    let bob = network.get_node("bob").unwrap();
    let bob_client: OnlineClient<PolkadotConfig> = bob.wait_client().await.unwrap();

    let charlie = network.get_node("charlie").unwrap();
    let charlie_client: OnlineClient<PolkadotConfig> = charlie.wait_client().await.unwrap();

    wait_n_blocks(&alice_client, 5, "alice").await;
    wait_n_blocks(&bob_client, 5, "bob").await;
    wait_n_blocks(&charlie_client, 5, "charlie").await;
}

async fn wait_n_blocks(client: &OnlineClient<PolkadotConfig>, n: usize, name: &str) {
    let mut blocks = client.blocks().subscribe_finalized().await.unwrap().take(n);

    while let Some(block) = blocks.next().await {
        println!("{name} Block #{}", block.unwrap().header().number);
    }
}
