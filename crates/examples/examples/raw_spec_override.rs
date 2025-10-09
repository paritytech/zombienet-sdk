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

    let raw_spec_override = json!({
       "name": "overridden-name",
    });

    let _network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_raw_spec_override(raw_spec_override)
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await
        .unwrap();

    println!("🚀🚀🚀🚀 network deployed");

    // The overridden spec can be verified by checking the logs of any node.

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}
