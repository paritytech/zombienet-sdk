use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|relay| {
            relay
                .with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.4.0")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|para| {
            para.with_id(2000)
                .cumulus_based(true)
                .evm_based(true)
                .with_collator(|collator| {
                    collator
                        .with_name("evm-collator")
                        .with_command("polkadot-parachain")
                        .with_override_eth_key("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                        .with_image("docker.io/parity/polkadot-parachain:latest")
                })
        })
        .build()
        .expect("errored?")
        .spawn_k8s()
        .await?;

    println!("🚀 network with EVM-based parachain is up");

    let node = network.get_node("alice")?;

    let role = node.reports("node_roles").await?;
    println!("Role is {role}");

    Ok(())
}
