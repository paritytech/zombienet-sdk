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
bootnodes = ["/ip4/127.0.0.1/tcp/30333/p2p/12D3KooW..."]
```

### Builder

```rust
use zombienet_sdk::{NetworkConfigBuilder, GlobalSettingsBuilder};

let settings = GlobalSettingsBuilder::new()
    .with_network_spawn_timeout(3600)
    .with_node_spawn_timeout(600)
    .with_local_ip("127.0.0.1")
    .with_base_dir("/tmp/zombienet")
    .with_spawn_concurrency(4)
    .build()
    .unwrap();

let config = NetworkConfigBuilder::new()
    .with_global_settings(|_| settings.clone())
    .build();
```

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `timeout` | Number | 3600 | Global timeout (seconds) for network spawn |
| `node_spawn_timeout` | Number | 600 | Individual node spawn timeout (seconds). Override with `ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS` env var |
| `local_ip` | String | `127.0.0.1` | Local IP for exposed services |
| `base_dir` | String | Random temp dir | Base directory for network artifacts |
| `spawn_concurrency` | Number | 100 | Number of concurrent spawn processes. Override with `ZOMBIE_SPAWN_CONCURRENCY` env var |
| `tear_down_on_failure` | Boolean | `true` | Tear down network if nodes become unresponsive |
| `bootnodes` | Array | — | External bootnode multiaddresses |
