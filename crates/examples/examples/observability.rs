//! Example: Observability add-on (Prometheus + Grafana)
//!
//! This example demonstrates two ways to use the observability stack:
//!
//! 1. **From config**: Include `[settings.observability]` in TOML so the stack
//!    starts automatically when the network spawns.
//!
//! 2. **As an add-on**: Call `network.start_observability()` on any running
//!    network, including one re-attached via `attach_to_live`.
//!
//! Requirements: Docker or Podman must be available on the host.
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|n| n.with_name("alice"))
                .with_validator(|n| n.with_name("bob"))
        })
        .with_global_settings(|s| {
            s.with_observability(|o| {
                o.with_enabled(true)
                    .with_prometheus_port(9090)
                    .with_grafana_port(3000)
            })
        })
        .build()
        .expect("Failed to build network config");

    println!("🚀 Spawning network with observability...");

    let network = config.spawn_native().await?;
    if let Some(obs) = network.observability() {
        println!("📊 Prometheus: {}", obs.prometheus_url);
        println!("📊 Grafana:    {}", obs.grafana_url);
    }

    tokio::signal::ctrl_c().await?;

    let _ = network.destroy().await;

    Ok(())
}
