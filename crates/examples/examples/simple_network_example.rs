use configuration::NetworkConfig;

fn main() {

    let load_from_toml =
            NetworkConfig::load_from_toml("./0001-simple.toml").unwrap();

    // let config = NetworkConfigBuilder::new()
    //     .with_relaychain(|r| {
    //         r.with_chain("rococo-local")
    //             .with_node(|node| node.with_name("alice").with_command("polkadot"))
    //     })
    //     .build();

    println!("{:?}", load_from_toml);
}
