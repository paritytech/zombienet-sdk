use std::{
    env,
    pin::Pin,
    time::{Duration, Instant},
};

use configuration::{NetworkConfig, NetworkConfigBuilder};
use futures::{stream::StreamExt, Future};
use orchestrator::{AddCollatorOptions, AddNodeOptions};
use serde_json::json;
use support::fs::local::LocalFileSystem;
use zombienet_sdk::{Network, NetworkConfigExt, OrchestratorError, PROVIDERS};

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.7.0")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator")
                    .with_command("polkadot-parachain")
                    .with_image("docker.io/parity/polkadot-parachain:1.7.0")
            })
        })
        .build()
        .unwrap()
}

type SpawnResult = Result<Network<LocalFileSystem>, OrchestratorError>;
fn get_spawn_fn() -> fn(NetworkConfig) -> Pin<Box<dyn Future<Output = SpawnResult> + Send>> {
    const PROVIDER_KEY: &str = "ZOMBIE_PROVIDER";
    let provider = env::var(PROVIDER_KEY).unwrap_or(String::from("k8s"));
    assert!(
        PROVIDERS.contains(&provider.as_str()),
        "\n❌ Invalid provider, available options {}\n",
        PROVIDERS.join(", ")
    );

    // TODO: revisit this

    if provider == "k8s" {
        zombienet_sdk::NetworkConfig::spawn_k8s
    } else {
        zombienet_sdk::NetworkConfig::spawn_native
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ci_k8s_basic_functionalities_should_works() {
    tracing_subscriber::fmt::init();
    let now = Instant::now();

    let config = small_network();
    let spawn_fn = get_spawn_fn();

    let mut network = spawn_fn(config).await.unwrap();
    // Optionally detach the network
    // network.detach().await;

    let elapsed = now.elapsed();
    println!("🚀🚀🚀🚀 network deployed in {:.2?}", elapsed);

    // give some time to node bootstrap
    tokio::time::sleep(Duration::from_secs(3)).await;
    // Get a ref to the node
    let alice = network.get_node("alice").unwrap();

    let role = alice.reports("node_roles").await.unwrap();
    println!("Role is {role}");
    assert_eq!(role, 4.0);

    // subxt
    let client = alice.client::<subxt::PolkadotConfig>().await.unwrap();

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

    assert!(best_block >= 2.0, "Current best {}", best_block);

    // pjs
    let para_is_registered = r#"
    const paraId = arguments[0];
    const parachains: number[] = (await api.query.paras.parachains()) || [];
    const isRegistered = parachains.findIndex((id) => id.toString() == paraId.toString()) >= 0;
    return isRegistered;
    "#;

    let is_registered = alice
        .pjs(para_is_registered, vec![json!(2000)], None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(is_registered, json!(true));

    // run pjs with code
    let query_paras = r#"
    const parachains: number[] = (await api.query.paras.parachains()) || [];
    return parachains.toJSON()
    "#;

    let paras = alice.pjs(query_paras, vec![], None).await.unwrap();

    println!("parachains registered: {:?}", paras);

    // collator
    let collator = network.get_node("collator").unwrap();
    let client = collator.client::<subxt::PolkadotConfig>().await.unwrap();

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

    // tear down (optional if you don't detach the network)
    // network.destroy().await.unwrap();
}
