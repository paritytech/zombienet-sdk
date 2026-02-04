use std::time::Duration;

use futures::stream::StreamExt;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{subxt, NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let script_path = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/monitor-blocks.sh");

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|v| {
                    v.with_name("alice").with_rpc_port(9944).with_args(vec![
                        "--alice".into(),                   // Flag
                        ("--rpc-methods", "unsafe").into(), // Option (K/V)
                    ])
                })
                .with_validator(|v| {
                    v.with_name("bob").with_args(vec![
                        "--bob".into(),               // Flag
                        ("--rpc-cors", "all").into(), // Option (K/V)
                    ])
                })
        })
        .with_custom_process(|c| {
            c.with_name("block-monitor")
                .with_command("bash")
                .with_args(vec![
                    // Positional arguments - script path and WebSocket URL
                    script_path.into(),
                    "ws://127.0.0.1:9944".into(),
                    "5".into(), // Monitor 5 blocks
                ])
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("\n🚀 Network deployed successfully!");

    // Wait for alice node and get client
    let alice = network.get_node("alice")?;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;

    // Wait for 3 finalized blocks
    let mut blocks = client.blocks().subscribe_finalized().await?.take(5);

    println!("\n⏲  Waiting for 5 finalized relay chain blocks:");
    while let Some(block) = blocks.next().await {
        println!("  Block #{}", block?.header().number);
    }

    println!("\n✅ Block production verified!");

    Ok(())
}
