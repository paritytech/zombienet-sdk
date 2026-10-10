use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{
    CustomProcessState, DependencyCondition, NetworkConfigBuilder, NetworkConfigExt, ReadyCheck,
};

// Custom processes with a lifecycle: a one-shot that prepares a directory, a
// server that starts once the one-shot completed and is ready when its http
// check passes, and a client that starts once the server is ready.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
        })
        // runs to completion: exit 0 is what `files` waits for
        .with_custom_process(|c| {
            c.with_name("prepare")
                .with_command("sh")
                .with_args(vec![(
                    "-c",
                    "mkdir -p /tmp/zombie-lifecycle && echo hello > /tmp/zombie-lifecycle/index.html",
                )
                    .into()])
                .with_one_shot()
        })
        // ready once GET / answers on its port, within a minute
        .with_custom_process(|c| {
            c.with_name("files")
                .with_command("python3")
                .with_args(vec![
                    ("-m", "http.server").into(),
                    ("--bind", "127.0.0.1").into(),
                    ("--directory", "/tmp/zombie-lifecycle").into(),
                    "{{ZOMBIE:files:port_http}}".into(),
                ])
                .with_named_port("http", 0)
                .with_ready_check(ReadyCheck::http("http", "/"))
                .with_timeout(60)
                .with_dependency("prepare")
        })
        // starts once `files` is healthy, said explicitly here; a bare name
        // would mean the same, healthy being the natural condition of a process
        // with a check
        .with_custom_process(|c| {
            c.with_name("reader")
                .with_command("sh")
                .with_args(vec![("-c", "echo reading; sleep 600").into()])
                .with_dependency_on("files", DependencyCondition::ServiceHealthy)
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    for process in network.custom_processes() {
        println!("{:<8} {:?}", process.name(), process.state());
    }
    for skipped in network.skipped_custom_processes() {
        println!("{:<8} skipped: {}", skipped.name, skipped.reason);
    }
    assert_eq!(
        network.get_custom_process("prepare")?.state(),
        CustomProcessState::Completed
    );
    assert_eq!(
        network.get_custom_process("files")?.state(),
        CustomProcessState::Ready
    );

    Ok(())
}
