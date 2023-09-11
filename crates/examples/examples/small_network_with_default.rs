use configuration::NetworkConfigBuilder;
use orchestrator::Orchestrator;

fn main() {
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("name"))
        })
        .build();

    println!("{:?}", config.unwrap());
}
