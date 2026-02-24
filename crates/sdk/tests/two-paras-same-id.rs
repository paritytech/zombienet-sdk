use zombienet_sdk::{environment::get_spawn_fn, NetworkConfigBuilder};

#[tokio::test(flavor = "multi_thread")]
async fn ci_two_paras_same_id() {
    tracing_subscriber::fmt::init();
    let spawn_fn = get_spawn_fn();
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.7.0")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_default_command("polkadot-parachain")
                .with_default_image("docker.io/parity/polkadot-parachain:1.7.0")
                .with_collator(|n| n.with_name("collator"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_default_command("polkadot-parachain")
                .with_default_image("docker.io/parity/polkadot-parachain:1.7.0")
                .with_registration_strategy(zombienet_sdk::RegistrationStrategy::Manual)
                .with_collator(|n| n.with_name("collator1"))
        })
        .build()
        .unwrap();

    let network = spawn_fn(config).await.unwrap();

    assert!(network.get_node("collator").is_ok());
    assert!(network.get_node("collator1").is_ok());

    // First parachain (out of two) is fetched
    assert_eq!(network.parachain(2000).unwrap().unique_id(), "2000");

    // First and second parachain hav the same para_id
    assert_eq!(
        network.parachain_by_unique_id("2000").unwrap().para_id(),
        network.parachain_by_unique_id("2000-1").unwrap().para_id(),
    );
}
