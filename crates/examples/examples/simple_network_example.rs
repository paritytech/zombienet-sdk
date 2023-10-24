// use std::time::Duration;

use configuration::NetworkConfig;
use futures::stream::StreamExt;
use orchestrator::Orchestrator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = NetworkConfig::load_from_toml("./crates/examples/examples/0001-simple.toml")
        .expect("errored?");

    let network = Orchestrator::native().spawn(config).await?;
    println!("🚀🚀🚀🚀 network deployed");

    let client = network
        .get_node("alice")?
        .client::<subxt::PolkadotConfig>()
        .await?;
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    Ok(())
}
