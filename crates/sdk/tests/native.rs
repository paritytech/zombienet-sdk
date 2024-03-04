use std::time::Duration;

use configuration::NetworkConfigBuilder;
use tokio::{time::timeout, try_join};
use zombienet_sdk::NetworkConfigExt;

#[tokio::test(flavor = "multi_thread")]
async fn a_simple_relaychain_network_runs_correctly() {
    let _network = NetworkConfigBuilder::new()
        .with_relaychain(|relay| {
            relay
                .with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| {
                    node.with_name("alice")
                        .with_args(vec![("-lruntime=debug").into()])
                        .with_env(vec![("FOO", "BAR")])
                })
                .with_node(|node| {
                    node.with_name("bob")
                        .with_args(vec![("-lruntime=trace").into()])
                        .with_env(vec![("BAR", "BAZ")])
                })
        })
        .build()
        .unwrap()
        .spawn_native()
        .await
        .unwrap();

    let system = sysinfo::System::new_all();

    let alice_node_process = helpers::get_node_process(&system, "polkadot", "alice")
        .expect("alice node process to exists");
    let bob_node_process =
        helpers::get_node_process(&system, "polkadot", "bob").expect("bob node process to exists");

    // correct command is running
    assert!(matches!(alice_node_process.cmd().first(), Some(name) if name == "polkadot"));
    assert!(matches!(bob_node_process.cmd().first(), Some(name) if name == "polkadot"));

    // env variables are passed correctly
    assert!(alice_node_process
        .environ()
        .contains(&"FOO=BAR".to_string()));
    assert!(bob_node_process.environ().contains(&"BAR=BAZ".to_string()));

    // args are passed correctly
    assert!(alice_node_process.cmd().contains(&"-lruntime=debug".into()));
    assert!(bob_node_process.cmd().contains(&"-lruntime=trace".into()));

    // namespace directories exists locally
    let namespace_path = helpers::get_namespace_path(alice_node_process);
    assert!(namespace_path.exists());
    assert!(namespace_path.join("alice").exists());
    assert!(namespace_path.join("bob").exists());

    // logs exists and are updating for some time
    let alice_logs = namespace_path.join("alice").join("alice.log");
    let bob_logs = namespace_path.join("bob").join("bob.log");
    assert!(try_join!(
        timeout(
            Duration::from_secs(30),
            helpers::logs_exists_and_updating(alice_logs)
        ),
        timeout(
            Duration::from_secs(30),
            helpers::logs_exists_and_updating(bob_logs)
        )
    )
    .is_ok());

    // metrics endpoints are available and updating
    assert!(try_join!(
        timeout(
            Duration::from_secs(30),
            helpers::metrics_are_available_and_updating(alice_node_process)
        ),
        timeout(
            Duration::from_secs(30),
            helpers::metrics_are_available_and_updating(bob_node_process)
        )
    )
    .is_ok());

    // RPC is available
    assert!(try_join!(
        helpers::rpc_is_available_and_responsive(alice_node_process),
        helpers::rpc_is_available_and_responsive(bob_node_process),
    )
    .is_ok());
}

mod helpers {
    use std::{path::PathBuf, str::FromStr, time::Duration};

    use subxt::{OnlineClient, PolkadotConfig};
    use sysinfo::{Process, System};
    use tokio::time::sleep;

    pub(super) fn get_node_process<'a>(
        system: &'a System,
        process_name: &str,
        node_name: &str,
    ) -> Option<&'a Process> {
        system.processes_by_name(process_name).find(|process| {
            if let Some(arg_value) = get_arg_value(process, "--name") {
                return arg_value == node_name;
            }

            false
        })
    }

    pub(super) fn get_arg_value<'a>(process: &'a Process, arg_name: &str) -> Option<&'a String> {
        let mut args = process.cmd().into_iter().enumerate();

        args.find(|(_, arg)| arg.as_str() == arg_name)
            .and_then(|(index, _)| Some(&process.cmd()[index + 1]))
    }

    pub(super) fn get_namespace_path(process: &Process) -> PathBuf {
        let raw_path = get_arg_value(process, "--base-path").expect("--base-path to be defined");
        let path = PathBuf::from_str(raw_path).expect("--base-path to be a valid path");

        path.parent()
            .expect("data dir to be defined")
            .parent()
            .expect("namespace dir to be defined")
            .to_path_buf()
    }

    pub(super) async fn logs_exists_and_updating(logs_path: PathBuf) {
        let mut logs_update = 3;
        let mut last_logs_size = 0;

        while logs_update > 0 {
            sleep(Duration::from_millis(200)).await;

            if !logs_path.exists() {
                continue;
            }

            let logs_size = logs_path
                .metadata()
                .expect("metadata to be available")
                .len();

            if logs_size > last_logs_size {
                logs_update -= 1;
                last_logs_size = logs_size
            }
        }
    }

    pub(super) async fn metrics_are_available_and_updating(process: &Process) {
        let prometheus_port =
            get_arg_value(process, "--prometheus-port").expect("--prometheus-port to be defined");
        let metrics_uri = format!("http://127.0.0.1:{prometheus_port}/metrics");
        let client = reqwest::Client::new();

        let mut metrics_update = 3;
        let mut last_metrics = String::new();

        while metrics_update > 0 {
            sleep(Duration::from_millis(200)).await;

            match client.get(&metrics_uri).send().await {
                Ok(res) => {
                    let metrics = res.text().await.expect("metrics response to be decodable");

                    if metrics != last_metrics {
                        metrics_update -= 1;
                        last_metrics = metrics;
                    }
                },
                Err(_) => continue,
            }
        }
    }

    pub(super) async fn rpc_is_available_and_responsive(
        process: &Process,
    ) -> Result<(), subxt::Error> {
        let rpc_port = get_arg_value(process, "--rpc-port").expect("--rpc-port to be defined");

        OnlineClient::<PolkadotConfig>::from_insecure_url(format!("ws://127.0.0.1:{rpc_port}"))
            .await?
            .blocks()
            .at_latest()
            .await?
            .number();

        Ok(())
    }
}

