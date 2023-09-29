use std::time::Duration;

use configuration::NetworkConfigBuilder;
use orchestrator::{AddNodeOpts, Orchestrator};
use provider::NativeProvider;
use support::fs::local::LocalFileSystem;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap();

    let fs = LocalFileSystem;
    let provider = NativeProvider::new(fs.clone());
    let orchestrator = Orchestrator::new(fs, provider);
    let mut network = orchestrator.spawn(config).await?;
    println!("🚀🚀🚀🚀 network deployed");
    // add  a new node
    let opts = AddNodeOpts {
        rpc_port: Some(9444),
        is_validator: true,
        ..Default::default()
    };

    // TODO: add check to ensure if unique
    network.add_node("new1", opts, None).await?;

    tokio::time::sleep(Duration::from_secs(5)).await;

    // Example of some opertions that you can do
    // with `nodes` (e.g pause, resume, restart)
    // pause the node
    // network.pause_node("new1").await?;
    // println!("node new1 paused!");

    // tokio::time::sleep(Duration::from_secs(5)).await;

    // network.resume_node("new1").await?;
    // println!("node new1 resumed!");

    let col_opts = AddNodeOpts {
        command: Some("polkadot-parachain".try_into()?),
        ..Default::default()
    };
    network.add_node("new-col-1", col_opts, Some(100)).await?;
    println!("new collator deployed!");

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}

    // Ok(())
}
