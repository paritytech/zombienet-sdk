use std::{panic, pin::Pin};

use configuration::{NetworkConfig, NetworkConfigBuilder};
use futures::{stream::StreamExt, Future};
use k8s_openapi::api::core::v1::Namespace;
use kube::{api::DeleteParams, Api};
use serde_json::json;
use support::fs::local::LocalFileSystem;
use zombienet_sdk::{Network, NetworkConfigExt};

fn small_network() -> NetworkConfig {
    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.4.0")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator")
                    .with_command("test-parachain")
                    .with_image(
                    "docker.io/paritypr/test-parachain:c90f9713b5bc73a9620b2e72b226b4d11e018190",
                )
            })
        })
        .build()
        .unwrap()
}

pub fn run_k8s_test<T>(config: NetworkConfig, test: T)
where
    T: panic::UnwindSafe,
    T: FnOnce(Network<LocalFileSystem>) -> Pin<Box<dyn Future<Output = ()> + 'static + Send>>,
{
    use std::time::Instant;

    let mut ns_name: Option<String> = None;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            let now = Instant::now();

            #[allow(unused_mut)]
            let mut network = config.spawn_k8s().await.unwrap();

            let elapsed = now.elapsed();
            println!("🚀🚀🚀🚀 network deployed in {:.2?}", elapsed);

            // get ns name to cleanup if test fails
            ns_name = Some(network.ns_name());

            // run some tests on the newly started network
            test(network).await;
        })
    }));

    // IF we created a new namespace, allway cleanup
    if let Some(ns_name) = ns_name {
        // remove the ns
        runtime.block_on(async {
            let k8s_client = kube::Client::try_default().await.unwrap();
            let namespaces = Api::<Namespace>::all(k8s_client);

            _ = namespaces.delete(&ns_name, &DeleteParams::default()).await;
        })
    }

    assert!(result.is_ok());
}

#[test]
fn basic_functionalities_should_works() {
    tracing_subscriber::fmt::init();
    let config = small_network();
    run_k8s_test(config, |network| {
        Box::pin(async move {
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
                .pjs(para_is_registered, vec![json!(2000)])
                .await
                .unwrap()
                .unwrap();
            assert_eq!(is_registered, json!(true));

            // run pjs with code
            let query_paras = r#"
            const parachains: number[] = (await api.query.paras.parachains()) || [];
            return parachains.toJSON()
            "#;

            let paras = alice.pjs(query_paras, vec![]).await.unwrap();

            println!("parachains registered: {:?}", paras);

            // tear down
            network.destroy().await.unwrap();
        })
    });
}
