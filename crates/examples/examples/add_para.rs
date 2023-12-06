use anyhow::anyhow;
use futures::stream::StreamExt;
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let mut network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let alice = network.get_node("alice")?;
    let client = alice.client::<subxt::PolkadotConfig>().await?;

    // wait 3 blocks
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    println!("⚙️  adding parachain to the running network");

    let para_config = network
        .para_config_builder()
        .with_id(100)
        .with_default_command("polkadot-parachain")
        .with_collator(|c| c.with_name("col-100-1"))
        .build()
        .map_err(|_e| anyhow!("Building config"))?;

    network.add_parachain(&para_config, None).await?;

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}

    #[allow(clippy::unreachable)]
    #[allow(unreachable_code)]
    Ok(())
}
