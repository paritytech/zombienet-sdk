use std::time::Duration;

use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.4.0")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .cumulus_based(true)
                .with_collator(|n|
                    n.with_name("collator")
                    // TODO: check how we can clean
                    .with_command("polkadot-parachain")
                    // .with_command("test-parachain")
                    .with_image("docker.io/paritypr/test-parachain:c90f9713b5bc73a9620b2e72b226b4d11e018190")
                )
        })
        .build()
        .unwrap()
        .spawn_native()
        // .spawn_k8s()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");
    // give some time to node's bootstraping
    tokio::time::sleep(Duration::from_secs(12)).await;

    // // Add a new node to the running network.
    // let opts = AddNodeOptions {
    //     rpc_port: Some(9444),
    //     is_validator: true,
    //     ..Default::default()
    // };

    // network.add_node("new1", opts).await?;

    // Example of some operations that you can do
    // with `nodes` (e.g pause, resume, restart)

    // Get a ref to the node
    let node = network.get_node("alice")?;

    let is_10 = node.assert("block_height{status=\"best\"}", 10).await?;
    println!("is_10: {is_10}");

    // let role = node.reports("node_roles").await?;
    // println!("Role is {role}");

    // pause the node
    // node.pause().await?;
    // println!("node new1 paused!");

    // node.resume().await?;
    // println!("node new1 resumed!");

    // let col_opts = AddCollatorOptions {
    //     command: Some("polkadot-parachain".try_into()?),
    //     ..Default::default()
    // };
    // network.add_collator("new-col-1", col_opts, 100).await?;
    // println!("new collator deployed!");

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}

    // Ok(())
}
