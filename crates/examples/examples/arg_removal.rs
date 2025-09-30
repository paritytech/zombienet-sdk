use std::time::Duration;

use anyhow::anyhow;
use zombienet_sdk::NetworkConfigBuilder;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();

    let images = zombienet_sdk::environment::get_images_from_env();
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image(images.polkadot.as_str())
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| {
                    node.with_name("bob").with_args(vec![
                        // Use -: prefix to remove the default insecure validator flag
                        "-:--insecure-validator-i-know-what-i-do".into(),
                    ])
                })
        })
        .build()
        .map_err(|e| {
            let errs = e
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            anyhow!("config errs: {errs}")
        })?;

    let spawn_fn = zombienet_sdk::environment::get_spawn_fn();
    let network = spawn_fn(config).await?;

    println!("✅ Network spawned with secure validator 'bob'\n");

    tokio::time::sleep(Duration::from_secs(5)).await;

    let alice = network.get_node("alice")?;
    let bob = network.get_node("bob")?;

    println!("📊 Node Status:");
    println!("  - Alice (regular validator): {}", alice.name());
    println!("  - Bob (secure validator): {}", bob.name());

    Ok(())
}
