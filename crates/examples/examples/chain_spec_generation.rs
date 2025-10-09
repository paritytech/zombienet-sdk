use std::time::Duration;

use futures::stream::StreamExt;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{subxt, NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let chain_name = "rococo-local";
    let chain_spec_command = "polkadot build-spec --chain rococo-local --disable-default-bootnode";

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain(chain_name)
            // The command that will be executed to generate the chain spec.
            // The output of the command must be a valid chain spec in JSON format.
            // Here we create a file `rococo-local.json` and zombienet will use it as the chain spec.
            .with_chain_spec_command(chain_spec_command)
        // By default, the command is executed inside a container.
        // If you want to run it on your local machine, you can set this to true.
        .chain_spec_command_is_local(false)
        .with_default_command("polkadot")
        .with_validator(|v| v.with_name("alice"))
        .with_validator(|v| v.with_name("bob"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await
        .unwrap();

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

    Ok(())
}
