//! Example: Manual parachain registration in a running Zombienet network.
//!
//! This example demonstrates how to:
//! - Deploy a relaychain and parachain network
//! - Wait for finalized blocks
//! - Register a parachain manually
//! - Verify parachain block production after registration

use std::time::Duration;

use futures::stream::StreamExt;
use zombienet_sdk::{subxt, NetworkConfigBuilder, NetworkConfigExt, RegistrationStrategy};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();

    // let images = zombienet_sdk::environment::get_images_from_env();
    let mut network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_registration_strategy(RegistrationStrategy::Manual)
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let alice = network.get_node("alice")?;
    tokio::time::sleep(Duration::from_secs(10)).await;
    println!("{alice:#?}");
    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;

    // wait 3 blocks
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    println!("⏲  waiting for 3 finalized relay chain blocks");
    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    println!("⚙️  registering parachain in the running network");

    network.register_parachain(2000).await?;

    let collator = network.get_node("collator")?;
    tokio::time::sleep(Duration::from_secs(10)).await;
    println!("{collator:#?}");

    let client = collator.wait_client::<subxt::PolkadotConfig>().await?;

    // wait 3 blocks
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    println!("⏲  waiting for 3 finalized parachain blocks");
    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    Ok(())
}
