// use std::time::Duration;

use configuration::NetworkConfig;
use orchestrator::Orchestrator;
use provider::NativeProvider;
use support::fs::local::LocalFileSystem;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = NetworkConfig::load_from_toml("./crates/examples/examples/0001-simple.toml")
        .expect("errored?");

    let fs = LocalFileSystem;
    let provider = NativeProvider::new(fs.clone());
    let orchestrator = Orchestrator::new(fs, provider);
    orchestrator.spawn(config).await?;
    println!("🚀🚀🚀🚀 network deployed");
    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}

    // Ok(())
}
