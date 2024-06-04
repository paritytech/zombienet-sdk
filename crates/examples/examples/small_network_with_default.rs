use std::time::Duration;

use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let para_spec_file = "/var/folders/rz/1cyx7hfj31qgb98d8_cg7jwh0000gn/T/zombie-1bb45583-bc3f-40e0-95b9-55ba136eb8ed/2000-plain.json";
    // let spec_path = std::path::Path::new(&para_spec_file);
    // let spec_path = spec_path.canonicalize()?;
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:latest")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_default_command("polkadot-parachain")
                .cumulus_based(true)
                .with_chain_spec_path(para_spec_file)
                .with_collator(|n|
                    n.with_name("collator")
                    .with_image("docker.io/paritypr/test-parachain:c90f9713b5bc73a9620b2e72b226b4d11e018190")
                )
                .with_collator(|n|
                    n.with_name("collator1")
                    .with_image("docker.io/paritypr/test-parachain:c90f9713b5bc73a9620b2e72b226b4d11e018190")
                )
                .with_collator(|n|
                    n.with_name("collator2")
                    .with_image("docker.io/paritypr/test-parachain:c90f9713b5bc73a9620b2e72b226b4d11e018190")
                )
        })
        .build()
        .unwrap()
        .spawn_native()
        // .spawn_k8s()
        // .spawn_docker()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");
    // give some time to node's bootstraping
    tokio::time::sleep(Duration::from_secs(120)).await;

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
    // let node = network.get_node("alice")?;

    // let is_10 = node.assert("block_height{status=\"best\"}", 10).await?;
    // println!("is_10: {is_10}");

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
