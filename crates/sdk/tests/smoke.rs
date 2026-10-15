use std::{path::PathBuf, time::Instant};

use configuration::{NetworkConfig, NetworkConfigBuilder, ProcessEnvVar, TransferedFile};
use futures::{stream::StreamExt, try_join};
use k8s_openapi::api::core::v1::Secret;
use kube::api::{Api, PostParams};
use orchestrator::{AddCollatorOptions, AddNodeOptions};
use zombienet_sdk::{
    environment::{get_attach_fn, get_provider_from_env, get_spawn_fn, Provider},
    CustomProcessState,
};

/// What the `secret-reader` process must find in `TURN_SECRET`, put in the
/// Secret `turn` of the namespace before the network is spawned.
const SECRET_VALUE: &str = "only-in-the-namespace";

/// On kubernetes `TURN_SECRET` comes from the Secret `turn`; docker has no
/// Secrets, so there it is the plain value and only the file is exercised.
fn turn_secret_env() -> ProcessEnvVar {
    match get_provider_from_env() {
        Provider::K8s => ProcessEnvVar::from_secret("TURN_SECRET", "turn", "secret"),
        _ => ("TURN_SECRET", SECRET_VALUE).into(),
    }
}

/// The Secret the custom process reads, created ahead of the spawn in the
/// namespace CI hands the test.
async fn create_secret(namespace: &str) {
    let client = kube::Client::try_default().await.unwrap();
    let mut secret = Secret::default();
    secret.metadata.name = Some("turn".into());
    secret.string_data = Some([("secret".to_string(), SECRET_VALUE.to_string())].into());
    match Api::<Secret>::namespaced(client, namespace)
        .create(&PostParams::default(), &secret)
        .await
    {
        Ok(_) => {},
        Err(kube::Error::Api(err)) if err.code == 409 => {},
        Err(err) => panic!("creating the Secret in {namespace}: {err}"),
    }
}

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.20.2")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
                .with_validator(|node| node.with_name("charlie"))
                .with_validator(|node| node.with_name("dave"))
        })
        .with_parachain(|p| {
            p.with_id(2000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator")
                    .with_command("polkadot-parachain")
                    .with_image("docker.io/parity/polkadot-parachain:1.7.0")
            })
        })
        .with_parachain(|p| {
            p.with_id(3000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator-new")
                    .with_command("polkadot-parachain")
                    .with_image("docker.io/parity/polkadot-parachain:v1.20.2")
            })
        })
        .with_parachain(|p| {
            p.with_id(2001)
                .with_default_image("docker.io/paritypr/test-parachain:master-d0393007")
                .with_collator(|n| n.with_name("collator-test").with_command("test-parachain"))
        })
        // A one-shot reading an env value from the Secret and a file mounted
        // from a ConfigMap, both in place before it starts.
        .with_custom_process(|c| {
            let hello = PathBuf::from("/tmp/zombie-1-hello.txt");
            std::fs::write(&hello, "hello from a ConfigMap\n").unwrap();
            let check = format!("test \"$TURN_SECRET\" = \"{SECRET_VALUE}\" && cat /opt/hello.txt");
            c.with_name("secret-reader")
                .with_command("sh")
                .with_image("docker.io/library/alpine:3.20")
                .with_args(vec!["-c".into(), check.as_str().into()])
                .with_env(vec![turn_secret_env()])
                .with_injected_file(TransferedFile::new(hello, "/opt/hello.txt"))
                .with_one_shot()
        })
        .with_global_settings(|g| {
            g.with_base_dir(PathBuf::from("/tmp/zombie-1"))
                .with_tear_down_on_failure(false)
        })
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_k8s_basic_functionalities_should_works() {
    let _ = tracing_subscriber::fmt::try_init();

    const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
    let now = Instant::now();

    let config = small_network();
    let spawn_fn = get_spawn_fn();

    if matches!(get_provider_from_env(), Provider::K8s) {
        create_secret(&std::env::var("ZOMBIE_K8S_CI_NAMESPACE").unwrap()).await;
    }
    let network = spawn_fn(config).await.unwrap();

    let elapsed = now.elapsed();
    println!("🚀🚀🚀🚀 network deployed in {elapsed:.2?}");

    let reader = network.get_custom_process("secret-reader").unwrap();
    assert_eq!(reader.state(), &CustomProcessState::Completed);
    let logs = reader.core().logs().await.unwrap();
    assert!(
        logs.contains("hello from a ConfigMap"),
        "secret-reader logs: {logs}"
    );

    // detach and attach to running
    network.detach().await;
    drop(network);
    let attach_fn = get_attach_fn();
    let zombie_path = PathBuf::from("/tmp/zombie-1/zombie.json");
    let mut network = attach_fn(zombie_path).await.unwrap();

    // Get a ref to the node
    let alice = network.get_node("alice").unwrap();

    let (_best_block_pass, client) = try_join!(
        alice.wait_metric(BEST_BLOCK_METRIC, |x| x > 5_f64),
        alice.wait_client::<subxt::PolkadotConfig>()
    )
    .unwrap();

    alice
        .wait_log_line_count("*rted #1*", true, 10)
        .await
        .unwrap();

    // check best block through metrics with timeout
    assert!(alice
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |x| x > 10_f64, 45_u32)
        .await
        .is_ok());

    // ensure timeout error
    let best_block = alice.reports(BEST_BLOCK_METRIC).await.unwrap();
    let res = alice
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |x| x > (best_block * 2_f64), 10_u32)
        .await;

    assert!(res.is_err());

    // get single metric
    let role = alice.reports("node_roles").await.unwrap();
    println!("Role is {role}");
    assert_eq!(role, 4.0);

    // subxt
    // wait 3 blocks
    let mut blocks = client.blocks().subscribe_finalized().await.unwrap().take(3);
    while let Some(block) = blocks.next().await {
        println!("Block #{}", block.unwrap().header().number);
    }

    // drop the client
    drop(client);

    // check best block through metrics
    let best_block = alice
        .reports("block_height{status=\"best\"}")
        .await
        .unwrap();

    assert!(best_block >= 2.0, "Current best {best_block}");

    // collator
    let collator = network.get_node("collator").unwrap();
    let client = collator
        .wait_client::<subxt::PolkadotConfig>()
        .await
        .unwrap();

    // wait 3 blocks
    let mut blocks = client.blocks().subscribe_finalized().await.unwrap().take(3);
    while let Some(block) = blocks.next().await {
        println!("Block (para) #{}", block.unwrap().header().number);
    }

    // add node
    let opts = AddNodeOptions {
        rpc_port: Some(9444),
        is_validator: true,
        ..Default::default()
    };

    network.add_node("new1", opts).await.unwrap();

    // add collator
    let col_opts = AddCollatorOptions {
        command: Some("polkadot-parachain".try_into().unwrap()),
        image: Some(
            "docker.io/parity/polkadot-parachain:1.7.0"
                .try_into()
                .unwrap(),
        ),
        ..Default::default()
    };

    network
        .add_collator("new-col-1", col_opts, 2000)
        .await
        .unwrap();

    // ensure zombie.json is updated with "new1" and "new-col-1"
    let raw = tokio::fs::read_to_string("/tmp/zombie-1/zombie.json")
        .await
        .unwrap();
    let zombie_json: serde_json::Value = serde_json::from_str(&raw).unwrap();

    assert!(zombie_json["parachains"]["2000"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| {
            p["collators"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["name"] == "new-col-1")
        }));

    assert!(zombie_json["relay"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "new1"));

    // pause / resume
    let alice = network.get_node("alice").unwrap();
    alice.pause().await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let res_err = alice
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |x| x > 5_f64, 5_u32)
        .await;

    assert!(res_err.is_err());

    alice.resume().await.unwrap();
    alice
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |x| x > 5_f64, 5_u32)
        .await
        .unwrap();

    // timeout connecting ws
    let collator = network.get_node("collator").unwrap();
    collator.pause().await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let r = collator
        .wait_client_with_timeout::<subxt::PolkadotConfig>(1_u32)
        .await;
    assert!(r.is_err());

    // tear down (optional if you don't detach the network)
    network.destroy().await.unwrap();
}
