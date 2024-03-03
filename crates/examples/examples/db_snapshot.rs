use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let mut _network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("substrate-node")
                .with_default_image("docker.io/paritypr/substrate:3428-e5be9c93")
                .with_default_db_snapshot("https://storage.googleapis.com/zombienet-db-snaps/substrate/0001-basic-warp-sync/chains-9677807d738b951e9f6c82e5fd15518eb0ae0419.tgz")
                .with_chain_spec_path("/Users/pepo/parity/polkadot-sdk/substrate/zombienet/0001-basic-warp-sync/chain-spec.json")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
                .with_node(|node| node.with_name("charlie"))
        })
        .build()
        .unwrap()
        // .spawn_native()
        .spawn_k8s()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}
}
