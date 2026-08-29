# Global Settings

### TOML

```toml
[settings]
timeout = 3600
node_spawn_timeout = 600
local_ip = "127.0.0.1"
base_dir = "/tmp/zombienet"
spawn_concurrency = 4
tear_down_on_failure = true
bootnodes_addresses = ["/ip4/127.0.0.1/tcp/30333/p2p/12D3KooW..."]

[settings.observability]
enabled = true
prometheus_port = 9090
grafana_port = 3000
prometheus_image = "prom/prometheus:latest"
grafana_image = "grafana/grafana:latest"
```

### Builder

```rust
use zombienet_sdk::NetworkConfigBuilder;

let config = NetworkConfigBuilder::new()
    .with_relaychain(|r| {
        r.with_chain("rococo-local")
            .with_default_command("polkadot")
            .with_validator(|v| v.with_name("alice"))
    })
    .with_global_settings(|gs| {
        gs.with_network_spawn_timeout(3600)
            .with_node_spawn_timeout(600)
            .with_local_ip("127.0.0.1")
            .with_base_dir("/tmp/zombienet")
            .with_spawn_concurrency(4)
            .with_observability(|obs| {
                obs.with_enabled(true)
                    .with_prometheus_port(9090)
                    .with_grafana_port(3000)
            })
    })
    .build()
    .unwrap();
```

### Observability

Set `[settings.observability]` to start a local Prometheus + Grafana stack after the network is up. The SDK writes a Prometheus config under the network base directory, points it at every node's Prometheus metrics endpoint, provisions Grafana with Prometheus as the default datasource, and returns the local URLs through `network.observability()`.

The same stack can also be started as an add-on after a network is already running:

```rust
use configuration::ObservabilityConfigBuilder;

let obs_config = ObservabilityConfigBuilder::new()
    .with_enabled(true)
    .with_grafana_port(3000)
    .build();

let info = network.start_observability(&obs_config).await?;
println!("Prometheus: {}", info.prometheus_url);
println!("Grafana: {}", info.grafana_url);
```

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `timeout` | Number | 3600 | Global timeout (seconds) for network spawn |
| `node_spawn_timeout` | Number | 600 | Individual node spawn timeout (seconds). Override with `ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS` env var |
| `local_ip` | String | `127.0.0.1` | Local IP for exposed services |
| `base_dir` | String | Random temp dir | Base directory for network artifacts |
| `spawn_concurrency` | Number | — | Number of concurrent spawn processes. Override with `ZOMBIE_SPAWN_CONCURRENCY` env var |
| `tear_down_on_failure` | Boolean | `true` | Tear down network if nodes become unresponsive |
| `bootnodes_addresses` | Array | — | External bootnode multiaddresses |
| `observability.enabled` | Boolean | `false` | Start the local Prometheus + Grafana stack after the network spawns |
| `observability.prometheus_port` | Number | Auto | Host port for Prometheus |
| `observability.grafana_port` | Number | Auto | Host port for Grafana |
| `observability.prometheus_image` | String | `prom/prometheus:latest` | Container image used for Prometheus |
| `observability.grafana_image` | String | `grafana/grafana:latest` | Container image used for Grafana |
