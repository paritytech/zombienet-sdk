//! Example demonstrating custom chain spec session key types.
//!
//! This example shows how to customize the session keys that are injected into the chain spec
//! for each validator node. This is useful when you need specific key types with specific
//! cryptographic schemes for your runtime.
//!
//! # Chain Spec Key Types
//!
//! There are 2 ways to specify key types:
//!
//! - **Short form**: `aura` - uses the predefined schema for the key type
//! - **Long form**: `aura_sr` - uses the explicitly specified schema (sr, ed, ec)
//!
//! ## Schemas
//!
//! - `sr` - Sr25519
//! - `ed` - Ed25519
//! - `ec` - ECDSA
//!
//! ## Predefined Key Type Schemas
//!
//! | Key Type | Default Schema | Description |
//! |----------|---------------|-------------|
//! | `babe` | sr | BABE consensus |
//! | `im_online` | sr | I'm Online |
//! | `parachain_validator` | sr | Parachain validator |
//! | `authority_discovery` | sr | Authority discovery |
//! | `para_validator` | sr | Para validator |
//! | `para_assignment` | sr | Para assignment |
//! | `aura` | sr (ed for asset-hub-polkadot) | AURA consensus |
//! | `nimbus` | sr | Nimbus consensus |
//! | `vrf` | sr | VRF |
//! | `grandpa` | ed | GRANDPA finality |
//! | `beefy` | ec | BEEFY |
//!
//! # Usage
//!
//! ```ignore
//! .with_validator(|node| {
//!     node.with_name("alice")
//!         // Only inject aura and grandpa keys into chain spec
//!         .with_chain_spec_key_types(vec!["aura", "grandpa"])
//! })
//! .with_validator(|node| {
//!     node.with_name("bob")
//!         // Override grandpa to use sr25519 instead of ed25519
//!         .with_chain_spec_key_types(vec!["aura", "grandpa_sr", "babe"])
//! })
//! ```
use futures::StreamExt;
use zombienet_sdk::{subxt, NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let keys = vec![
        "babe",                // default sr scheme
        "grandpa",             // default ed scheme
        "im_online",           // default sr scheme
        "authority_discovery", // default sr scheme
        "para_validator",      // default sr scheme
        "para_assignment",     // default sr scheme
        "beefy",               // default ec scheme
        // add a custom key type with explicit scheme
        "custom_ec", // custom key with ecdsa scheme
    ];
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.20.2")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| {
                    node.with_name("bob")
                        .with_chain_spec_key_types(keys.clone())
                })
                .with_validator(|node| {
                    node.with_name("charlie")
                        .with_chain_spec_key_types(keys.clone())
                })
        })
        .build()
        .unwrap()
        .spawn_docker()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let base_dir = network
        .base_dir()
        .ok_or("Failed to get network base directory")?;

    println!("📁 Network base directory: {}", base_dir);
    println!();

    println!("📋 Chain spec key types configuration:");
    println!("  - alice: Default keys (all standard session keys)");
    println!("  - bob: Custom keys (babe, grandpa, im_online, authority_discovery)");
    println!("  - charlie: Custom keys with overrides (babe, grandpa_sr, custom_ec)");
    println!();

    let alice = network.get_node("alice")?;
    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;
    let mut finalized_blocks = client.blocks().subscribe_finalized().await?.take(3);

    println!("⏳ Waiting for finalized blocks...");

    while let Some(block) = finalized_blocks.next().await {
        println!("✅ Finalized block {}", block?.header().number);
    }

    network.destroy().await?;

    Ok(())
}
