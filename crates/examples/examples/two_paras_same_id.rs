//! Example: Two parachains with the same ID.
//!
//! This example demonstrates how to:
//! - Deploy two parachains having the same ID

use std::time::Duration;

use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let _network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_registration_strategy(zombienet_sdk::RegistrationStrategy::Manual)
                .with_collator(|n| n.with_name("collator1").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
