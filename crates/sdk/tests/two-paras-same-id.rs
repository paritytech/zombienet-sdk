use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::test(flavor = "multi_thread")]
async fn two_paras_same_id() {
    tracing_subscriber::fmt::init();
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_registration_strategy(zombienet_sdk::RegistrationStrategy::Manual)
                .with_collator(|n| n.with_name("collator1").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await
        .unwrap();

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
