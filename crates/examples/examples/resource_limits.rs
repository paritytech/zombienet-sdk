use zombienet_sdk::NetworkConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config =
        NetworkConfig::load_from_toml("./crates/examples/examples/configs/resource_limits.toml")?;

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
