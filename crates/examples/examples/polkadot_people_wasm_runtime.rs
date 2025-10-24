use anyhow::anyhow;
use zombienet_sdk::{environment::get_spawn_fn, NetworkConfigBuilder};

const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();

    let config = NetworkConfigBuilder::new()
        .with_relaychain(|relaychain| {
            relaychain
                .with_chain("polkadot-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:latest")
                .with_chain_spec_runtime("https://github.com/polkadot-fellows/runtimes/releases/download/v1.9.3/polkadot_runtime-v1009003.compact.compressed.wasm", None)
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|parachain| {
            parachain
                .with_id(100)
                .with_chain("people-polkadot-local")
                .with_default_command("polkadot-omni-node")
                .with_default_image("docker.io/parity/polkadot-parachain:latest")
                .with_chain_spec_runtime("https://github.com/polkadot-fellows/runtimes/releases/download/v1.9.2/people-polkadot_runtime-v1009002.compact.compressed.wasm", None)
                .with_collator(|collator| collator.with_name("people-collator-1"))
                .with_collator(|collator| collator.with_name("people-collator-2"))
        })
        .build()
        .map_err(|e| {
            let errs = e
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            anyhow!("config errs: {errs}")
        })?;

    let spawn_fn = get_spawn_fn();
    let network = spawn_fn(config).await?;

    println!("🚀🚀🚀🚀 network deployed");

    // wait 2 blocks
    let alice = network.get_node("alice").unwrap();
    assert!(alice
        .wait_metric(BEST_BLOCK_METRIC, |b| b > 2_f64)
        .await
        .is_ok());

    Ok(())
}
