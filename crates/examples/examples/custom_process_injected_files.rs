use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{CustomProcessState, InjectedFile, NetworkConfigBuilder, NetworkConfigExt};

// A custom process running a script injected from this repository: the file
// is in place before the process starts, and `{{ZOMBIE:greeter:root}}` is
// where its absolute paths are. Spawned on native, as the other custom
// process examples are.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/greet.sh");

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
        })
        .with_custom_process(|c| {
            c.with_name("greeter")
                .with_command("sh")
                .with_args(vec![
                    "{{ZOMBIE:greeter:root}}/opt/greet.sh".into(),
                    "{{ZOMBIE:alice:ws_uri}}".into(),
                ])
                .with_injected_file(InjectedFile::new(script, "/opt/greet.sh").with_mode("0755"))
                .with_one_shot()
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    let greeter = network.get_custom_process("greeter")?;
    println!("{} {:?}", greeter.name(), greeter.state());
    assert_eq!(greeter.state(), &CustomProcessState::Completed);

    Ok(())
}
