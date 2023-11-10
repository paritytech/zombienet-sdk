use futures::stream::StreamExt;
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt, RegistrationStrategy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // tracing_subscriber::fmt::init();
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .with_registration_strategy(RegistrationStrategy::InGenesis)
                .with_chain("contracts-parachain-local")
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("/Users/pepo/parity/substrate-contracts-node/target/release/substrate-contracts-node"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let client = network
        .get_node("collator")?
        .client::<subxt::PolkadotConfig>()
        .await?;
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    Ok(())
}
