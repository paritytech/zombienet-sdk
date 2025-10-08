use std::time::Duration;

use serde_json::json;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let expected_balance = 2_222_222_222u128;
    let genesis_patch = json!({
        "balances": {
            "balances": [
                [
                    "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
                    expected_balance
                ]
            ]
        }
    });

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .with_genesis_overrides(genesis_patch)
                .with_default_command("polkadot-parachain")
                .with_collator(|c| c.with_name("collator"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await
        .unwrap();

    println!("🚀🚀🚀🚀 network deployed");

    let collator = network.get_node("collator")?;
    // Query Collator's balance using pjs
    let query_balance = r#"
        const { data: balance } = await api.query.system.account('5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY');
        return balance.free.toString();
        "#;

    let balance = collator.pjs(query_balance, vec![], None).await?.unwrap();
    println!("Queried balance: {balance:?}");
    assert_eq!(balance, expected_balance.to_string());

    #[allow(clippy::empty_loop)]
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
