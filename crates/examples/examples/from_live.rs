use std::path::Path;

use anyhow::Ok;
use futures::stream::StreamExt;
use zombienet_sdk::{subxt, AttachToLive, AttachToLiveNetwork};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    // there must be a live network running with zombie.json under this path
    let zombie_json_path = Path::new("/tmp/zombie-1/zombie.json");
    let network = AttachToLiveNetwork::attach_native(zombie_json_path).await?;

    let alice = network.get_node("alice").unwrap();

    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;

    // wait 3 blocks
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    Ok(())
}
