use configuration::NetworkConfigBuilder;

fn main() {
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_node(|node| node.with_name("alice").with_command("polkadot"))
        })
        .build();

    println!("{:?}", config.unwrap());
}
