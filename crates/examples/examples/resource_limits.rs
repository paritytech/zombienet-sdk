//! Example: Query and verify resource limits for relaychain nodes.
//!
//! This example loads a TOML config with custom resource limits
//! and prints the CPU/memory settings for each node.

use zombienet_sdk::NetworkConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config =
        NetworkConfig::load_from_toml("./crates/examples/examples/0002-resource_limits.toml")?;

    for node in config.relaychain().nodes() {
        println!("Node: {}", node.name());
        if let Some(resources) = node.resources() {
            println!("  CPU Limit: {:?}", resources.limit_cpu());
            println!("  Memory Limit: {:?}", resources.limit_memory());
            println!("  CPU Request: {:?}", resources.request_cpu());
            println!("  Memory Request: {:?}", resources.request_memory());
        } else {
            println!("  No resource limits set.");
        }
    }

    Ok(())
}
