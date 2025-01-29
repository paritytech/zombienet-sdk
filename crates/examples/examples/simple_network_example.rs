use futures::stream::StreamExt;
use zombienet_sdk::{subxt, NetworkConfig, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let network = NetworkConfig::load_from_toml("./crates/examples/examples/0001-simple.toml")
        .expect("errored?")
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let client = network
        .get_node("collator01")?
        .wait_client::<subxt::PolkadotConfig>()
        .await?;
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    Ok(())
}
