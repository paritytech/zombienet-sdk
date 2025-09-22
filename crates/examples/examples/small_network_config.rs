use zombienet_sdk::NetworkConfigBuilder;

fn main() {
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_validator(|node| node.with_name("alice").with_command("polkadot"))
        })
        .build();

    println!("{:?}", config.unwrap());
}
