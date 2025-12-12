use std::{env, path::PathBuf};

use configuration::{NetworkConfig, NetworkConfigBuilder};
use subxt::{ext::futures::StreamExt, OnlineClient, PolkadotConfig};
use zombienet_sdk::NetworkConfigExt;

fn small_network() -> NetworkConfig {
    let runtime_path = PathBuf::from(env::var("RUNTIME_PATH").unwrap());

    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("polkadot-local")
                .with_default_command("polkadot")
                .with_default_args(vec!["-lparachain=debug,runtime=debug".into()])
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(3000)
                .cumulus_based(true)
                .with_chain_spec_runtime(runtime_path, None)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
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

    let mut blocks = alice_client
        .blocks()
        .subscribe_finalized()
        .await
        .unwrap()
        .take(10);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block.unwrap().header().number);
    }

    let collator = network.get_node("collator").unwrap();
    let collator_client: OnlineClient<PolkadotConfig> = collator.wait_client().await.unwrap();

    let mut blocks = collator_client
        .blocks()
        .subscribe_finalized()
        .await
        .unwrap()
        .take(10);

    while let Some(block) = blocks.next().await {
        println!("Parachain Block #{}", block.unwrap().header().number);
    }
}
