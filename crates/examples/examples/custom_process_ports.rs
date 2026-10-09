use std::time::Duration;

use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

// A custom process that declares a port: zombienet picks it, tells the process
// through a placeholder, and reports where it is reachable.
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
        .with_custom_process(|c| {
            c.with_name("files")
                .with_command("python3")
                .with_args(vec![
                    ("-m", "http.server").into(),
                    ("--bind", "0.0.0.0").into(),
                    "{{ZOMBIE:files:port_http}}".into(),
                ])
                .with_named_port("http", 0)
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    let files = network.get_custom_process("files")?;
    let addr = files.get_uri_for_name("http").unwrap();
    println!(
        "\n📂 files serves on {addr} (inside the network: {})",
        files.port("http").unwrap().internal
    );

    // The process is not waited on at spawn; give it a moment and connect.
    tokio::time::sleep(Duration::from_secs(3)).await;
    tokio::net::TcpStream::connect(addr).await?;
    println!("✅ reachable");

    Ok(())
}
