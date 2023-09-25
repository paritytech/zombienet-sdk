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
            p.with_id(100).cumulus_based(true).with_collator(|n| {
                n.with_name("collator").with_command("polkadot-parachain")
                //.with_command("adder-collator")
            })
        })
        .build()
        .unwrap();

    println!("{:?}", &config);

    let fs = LocalFileSystem;
    let provider = NativeProvider::new(fs.clone());
    let orchestrator = Orchestrator::new(fs, provider);
    let mut network = orchestrator.spawn(config).await?;
    println!("🚀🚀🚀🚀 network deployed");
    // add  a new node
    let mut opts = AddNodeOpts::default();
    opts.rpc_port = Some(9444);
    opts.is_validator = true;

    // TODO: add check to ensure if unique
    network.add_node("new1", opts, None).await?;

    tokio::time::sleep(Duration::from_secs(5)).await;

    // pause the node
    network.pause_node("new1").await?;
    println!("node new1 paused!");

    tokio::time::sleep(Duration::from_secs(5)).await;

    network.resume_node("new1").await?;
    println!("node new1 resumed!");

    let mut col_opts = AddNodeOpts::default();
    col_opts.command = Some("polkadot-parachain".try_into()?);
    network.add_node("new-col-1", col_opts, Some(100)).await?;
    println!("new collator deployed!");

    // For now let just loop....
    while true {}
    Ok(())
}
