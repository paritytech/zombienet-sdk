use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node_group(|g| {
                    g.with_count(3)
                        .with_base_node(|b| b.with_name("relay_group"))
                })
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(true)
                .with_default_command("polkadot-parachain")
                .with_collator_group(|g| {
                    g.with_count(3)
                        .with_base_node(|b| b.with_name("para_group"))
                })
                .with_collator_group(|f| {
                    f.with_count(2)
                        .with_base_node(|b| b.with_name("para_group-2"))
                })
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let nodes = network.relaychain().nodes();
    nodes.iter().for_each(|node| {
        println!("Relay node: {}", node.name());
    });

    let collators = network.parachains()[0].collators();
    collators.iter().for_each(|collator| {
        println!("Collator: {}", collator.name());
    });

    #[allow(clippy::empty_loop)]
    loop {}
}
