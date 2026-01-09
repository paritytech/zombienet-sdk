use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let genesis_cmd = format!(
        "undying-collator export-genesis-state --pov-size={} --pvf-complexity={}",
        10000, 1
    );
    let _network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(false)
                // Use a custom command with arguments to generate the genesis state
                .with_genesis_state_generator(genesis_cmd.as_str())
                .with_collator(|n| {
                    n.with_name("collator")
                        .with_command("undying-collator")
                })
        })
        .build()
        .expect("Failed to build network config")
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed with custom genesis state generator");

    // Keep the network running
    #[allow(clippy::empty_loop)]
    loop {}
}
