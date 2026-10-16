use std::time::Instant;

use configuration::{NetworkConfig, NetworkConfigBuilder, TransferedFile};
use zombienet_sdk::{environment::get_spawn_fn, CustomProcessState};

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.20.2")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .cumulus_based(true)
                .with_default_image("docker.io/parity/polkadot-parachain:v1.20.2")
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(3000)
                .cumulus_based(true)
                .with_default_image("docker.io/parity/polkadot-omni-node:v1.20.2")
                .with_chain_spec_runtime("https://github.com/polkadot-fellows/runtimes/releases/download/v1.9.2/asset-hub-polkadot_runtime-v1009002.compact.compressed.wasm", None)
                .with_collator(|n| n.with_name("collator-omni").with_command("polkadot-omni-node"))
        })
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_native_smoke_should_works() {
    let _ = tracing_subscriber::fmt::try_init();
    const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
    let now = Instant::now();
    let config = small_network();
    let spawn_fn = get_spawn_fn();

    let network = spawn_fn(config).await.unwrap();

    let elapsed = now.elapsed();
    println!("🚀🚀🚀🚀 network deployed in {elapsed:.2?}");

    network.wait_until_is_up(20).await.unwrap();

    let elapsed = now.elapsed();
    println!("✅✅✅✅ network is up in {elapsed:.2?}");

    // Get a ref to the node
    let alice = network.get_node("alice").unwrap();
    // wait 10 blocks
    alice
        .wait_metric(BEST_BLOCK_METRIC, |x| x > 9_f64)
        .await
        .unwrap();
}

/// A one-shot custom process that is a script injected before it starts, run
/// as its `command` by `remote_path` and told where alice is.
#[tokio::test(flavor = "multi_thread")]
async fn ci_native_custom_process_runs_an_injected_file() {
    let _ = tracing_subscriber::fmt::try_init();

    let dir = std::env::temp_dir().join(format!("zombie-injected-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("greet.sh");
    std::fs::write(&script, include_str!("../../examples/scripts/greet.sh")).unwrap();

    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_custom_process(|c| {
            c.with_name("greeter")
                .with_command("/opt/greet.sh")
                .with_args(vec!["{{ZOMBIE:alice:ws_uri}}".into()])
                .with_injected_file(TransferedFile::new(&script, "/opt/greet.sh").mode("0755"))
                .with_one_shot()
        })
        .build()
        .unwrap();

    let network = get_spawn_fn()(config).await.unwrap();

    let greeter = network.get_custom_process("greeter").unwrap();
    assert_eq!(greeter.state(), &CustomProcessState::Completed);
    let logs = greeter.core().logs().await.unwrap();
    assert!(
        logs.contains("hello from an injected script, alice is at ws://"),
        "greeter logs: {logs}"
    );

    network.destroy().await.unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}
