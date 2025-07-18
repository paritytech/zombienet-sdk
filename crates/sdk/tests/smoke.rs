use std::time::Instant;

use configuration::{NetworkConfig, NetworkConfigBuilder};
use futures::{stream::StreamExt, try_join};
use orchestrator::{AddCollatorOptions, AddNodeOptions};
#[cfg(feature = "pjs")]
use serde_json::json;
use zombienet_sdk::environment::get_spawn_fn;

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

#[tokio::test(flavor = "multi_thread")]
async fn ci_k8s_basic_functionalities_should_works() {
    tracing_subscriber::fmt::init();
    const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
    let now = Instant::now();

    let config = small_network();
    let spawn_fn = get_spawn_fn();

    let mut network = spawn_fn(config).await.unwrap();
    // Optionally detach the network
    // network.detach().await;

    let elapsed = now.elapsed();
    println!("🚀🚀🚀🚀 network deployed in {elapsed:.2?}");

    // Get a ref to the node
    let alice = network.get_node("alice").unwrap();

    // timeout connecting ws
    let c = network.get_node("collator").unwrap();
    let r = c
        .wait_client_with_timeout::<subxt::PolkadotConfig>(1_u32)
        .await;
    assert!(r.is_err());

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

    #[cfg(feature = "pjs")]
    {
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

        println!("parachains registered: {paras:?}");
    }

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

    // pause / resume
    let alice = network.get_node("alice").unwrap();
    alice.pause().await.unwrap();
    let res_err = alice
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |x| x > 5_f64, 5_u32)
        .await;

    assert!(res_err.is_err());

    alice.resume().await.unwrap();
    alice
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |x| x > 5_f64, 5_u32)
        .await
        .unwrap();

    // tear down (optional if you don't detach the network)
    // network.destroy().await.unwrap();
}
