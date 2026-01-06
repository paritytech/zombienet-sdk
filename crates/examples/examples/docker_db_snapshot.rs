use futures::StreamExt;
use zombienet_sdk::{subxt, NetworkConfigBuilder, NetworkConfigExt};

const SNAPSHOT_URL: &str = "https://storage.googleapis.com/zombienet-db-snaps/substrate/0001-basic-warp-sync/chains-9677807d738b951e9f6c82e5fd15518eb0ae0419.tgz";
const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.20.2")
                .with_default_db_snapshot(SNAPSHOT_URL)
                .with_validator(|node| node.with_name("alice").validator(true))
        })
        .build()
        .unwrap()
        .spawn_docker()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let alice = network.get_node("alice")?;

    alice
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |v| v >= 5_f64, 90_u32)
        .await
        .expect("node should produce blocks shortly after restoring the snapshot");

    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;
    let mut finalized_blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = finalized_blocks.next().await {
        println!("Finalized block {}", block?.header().number);
    }

    network.destroy().await?;

    Ok(())
}
