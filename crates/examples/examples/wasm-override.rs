use zombienet_sdk::NetworkConfigBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(true)
                .with_wasm_override("path/to/a/wasm/runtime.wasm")
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(200)
                .cumulus_based(false)
                .with_collator(|n| n.with_name("collator2").with_command("adder-collator"))
        })
        .build()
        .unwrap();

    for p in network.parachains() {
        println!(
            "Parachain ID: {}, wasm override: {:?}",
            p.id(),
            p.wasm_override()
        );
    }

    Ok(())
}
