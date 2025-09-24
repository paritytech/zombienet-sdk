use futures::stream::StreamExt;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use zombienet_sdk::{subxt, NetworkConfig, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let network =
        NetworkConfig::load_from_toml("./crates/examples/examples/0002-simple-group-nodes.toml")
            .expect("errored?")
            .spawn_native()
            .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let nodes = network.relaychain().nodes();
    assert_eq!(nodes.len(), 6);
    nodes.iter().for_each(|node| {
        println!("Node: {}", node.name());
    });

    let collators = network.parachains()[0].collators();
    assert_eq!(collators.len(), 3);
    collators.iter().for_each(|collator| {
        println!("Collator: {}", collator.name());
    });

    let client = network
        .get_node("collator_group-1")?
        .wait_client::<subxt::PolkadotConfig>()
        .await?;
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    Ok(())
}
