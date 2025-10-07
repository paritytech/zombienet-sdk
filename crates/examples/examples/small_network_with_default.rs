use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let _network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.4.0")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator")
                    .with_command("polkadot-parachain")
                    .with_image(
                    "docker.io/paritypr/test-parachain:c90f9713b5bc73a9620b2e72b226b4d11e018190",
                )
            })
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}
}
