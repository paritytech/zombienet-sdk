// use std::time::Duration;

use configuration::NetworkConfig;
use orchestrator::Orchestrator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = NetworkConfig::load_from_toml("./crates/examples/examples/0001-simple.toml")
        .expect("errored?");

    Orchestrator::native().spawn(config).await?;
    println!("🚀🚀🚀🚀 network deployed");
    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}

    // Ok(())
}
