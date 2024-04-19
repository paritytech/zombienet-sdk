use futures::stream::StreamExt;
use serde_json::json;
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let alice = network.get_node("alice")?;
    let client = alice.client::<subxt::PolkadotConfig>().await?;

    // wait 2 blocks
    let mut blocks = client.blocks().subscribe_finalized().await?.take(2);

    while let Some(block) = blocks.next().await {
        println!("Block #{}", block?.header().number);
    }

    // run pjs with code
    let query_paras = r#"
    const parachains: number[] = (await api.query.paras.parachains()) || [];
    return parachains.toJSON()
    "#;

    let paras = alice.pjs(query_paras, vec![], None).await??;

    println!("parachains registered: {:?}", paras);

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    // run pjs with file
    let _ = alice
        .pjs_file(
            format!("{}/{}", manifest_dir, "examples/pjs_transfer.js"),
            vec![json!("//Alice")],
            None,
        )
        .await?;

    Ok(())
}
