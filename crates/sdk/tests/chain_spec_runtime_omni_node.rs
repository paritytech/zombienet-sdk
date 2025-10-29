use futures::StreamExt;
use zombienet_sdk::{environment::get_spawn_fn, NetworkConfigBuilder};

const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";

#[tokio::test(flavor = "multi_thread")]
async fn rococo_local_with_omni_node_and_wasm_runtime() {
    let _ = tracing_subscriber::fmt::try_init();

    let config = NetworkConfigBuilder::new()
        .with_relaychain(|relaychain| {
            relaychain
                .with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:latest")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|parachain| {
            parachain
                .with_id(2000).cumulus_based(true)
                .with_chain("asset-hub-rococo-local")
                .with_default_command("polkadot-omni-node")
                .with_default_image("docker.io/parity/polkadot-parachain:latest")
                .with_chain_spec_runtime("https://github.com/polkadot-fellows/runtimes/releases/download/v1.9.2/asset-hub-polkadot_runtime-v1009002.compact.compressed.wasm", None                )
                .with_collator(|collator| collator.with_name("omni-collator-1"))
                .with_collator(|collator| collator.with_name("omni-collator-2"))
        })
        .build()
        .unwrap();

    let spawn_fn = get_spawn_fn();
    let network = spawn_fn(config).await.unwrap();

    println!("🚀🚀🚀🚀 network deployed");

    // wait 2 blocks
    let alice = network.get_node("alice").unwrap();
    assert!(alice
        .wait_metric(BEST_BLOCK_METRIC, |b| b > 2_f64)
        .await
        .is_ok());

    // omni-collator-1
    let collator = network.get_node("omni-collator-1").unwrap();
    let client = collator
        .wait_client::<subxt::PolkadotConfig>()
        .await
        .unwrap();

    // wait 1 blocks
    let mut blocks = client.blocks().subscribe_finalized().await.unwrap().take(1);
    while let Some(block) = blocks.next().await {
        println!(
            "Block (omni-collator-1) #{}",
            block.unwrap().header().number
        );
    }

    // omni-collator-2
    let collator = network.get_node("omni-collator-2").unwrap();
    let client = collator
        .wait_client::<subxt::PolkadotConfig>()
        .await
        .unwrap();

    // wait 1 blocks
    let mut blocks = client.blocks().subscribe_finalized().await.unwrap().take(1);
    while let Some(block) = blocks.next().await {
        println!(
            "Block (omni-collator-2) #{}",
            block.unwrap().header().number
        );
    }
}
