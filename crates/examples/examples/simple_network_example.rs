// use std::time::Duration;

use configuration::NetworkConfig;
use orchestrator::Orchestrator;
use provider::NativeProvider;
use support::{fs::local::LocalFileSystem, process::os::OsProcessManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = NetworkConfig::load_from_toml("./crates/examples/examples/0001-simple.toml")
        .expect("errored?");

    let fs = LocalFileSystem;
    let pm = OsProcessManager;
    let provider = NativeProvider::new(fs.clone(), pm);
    let orchestrator = Orchestrator::new(fs, provider);
    let network = orchestrator.spawn(config).await?;
    println!("🚀🚀🚀🚀 network deployed");

    let client = network.get_node("alice").unwrap().client();
    let mut blocks = client.blocks().subscribe_finalized().await?;
    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    Ok(())
}
