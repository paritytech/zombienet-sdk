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
        NetworkConfig::load_from_toml("./crates/examples/examples/0003-arg-removal.toml")?
            .spawn_docker()
            .await?;

    println!("🚀🚀🚀🚀 network deployed");

    // Verify network structure
    let nodes = network.relaychain().nodes();
    assert_eq!(nodes.len(), 2, "Expected 2 validators in the network");
    println!("\n✅ Network has {} validators", nodes.len());

    // Get nodes and verify they exist
    let alice = network.get_node("alice")?;
    let bob = network.get_node("bob")?;

    println!("\n📊 Node Status:");
    println!("  - Alice (regular validator): {}", alice.name());
    println!("  - Bob (secure validator): {}", bob.name());

    // Verify arg removal feature: bob should NOT have the insecure validator flag
    let alice_args = alice.args();
    let bob_args = bob.args();

    println!("\n🔍 Verifying argument removal feature:");
    println!("  Alice args count: {}", alice_args.len());
    println!("  Bob args count: {}", bob_args.len());

    // Alice should have the default insecure validator flag
    let alice_has_insecure_flag = alice_args
        .iter()
        .any(|arg| arg.contains("insecure-validator-i-know-what-i-do"));
    assert!(
        alice_has_insecure_flag,
        "Alice should have the default --insecure-validator-i-know-what-i-do flag"
    );
    println!("  ✅ Alice has --insecure-validator-i-know-what-i-do flag (expected)");

    // Bob should NOT have the insecure validator flag (it was removed with -: prefix)
    let bob_has_insecure_flag = bob_args
        .iter()
        .any(|arg| arg.contains("insecure-validator-i-know-what-i-do"));
    assert!(
        !bob_has_insecure_flag,
        "Bob should NOT have the --insecure-validator-i-know-what-i-do flag (it was removed)"
    );
    println!(
        "  ✅ Bob does NOT have --insecure-validator-i-know-what-i-do flag (removed successfully)"
    );

    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);

    println!("\n📦 Finalized blocks:");
    let block = blocks.next().await;
    if let Some(block) = block {
        println!("  Block #{}", block.unwrap().header().number)
    }

    Ok(())
}
