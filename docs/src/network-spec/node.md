# Node

Validators, full nodes, and collators share this configuration.

### TOML

```toml
[[relaychain.nodes]]
name = "alice"
command = "polkadot"
# subcommand = "node"  # Optional: for nested CLI commands
image = "parity/polkadot:latest"
validator = true
invulnerable = true
bootnode = false
initial_balance = 5000000000000
args = ["--alice", "-lruntime=debug"]
env = [
    { name = "RUST_LOG", value = "info" }
]

# Networking
ws_port = 9944
rpc_port = 9933
prometheus_port = 9615
p2p_port = 30333
# p2p_cert_hash = "..."  # Optional: libp2p WebRTC cert hash
bootnodes = ["/ip4/127.0.0.1/tcp/30333/p2p/12D3KooW..."]

# Storage & Keys
db_snapshot = "/path/to/snapshot.tar.gz"
keystore_path = "/path/to/keystore"
keystore_key_types = ["aura", "gran"]
# override_eth_key = "0x..."  # Optional: override EVM session key
log_path = "/tmp/alice.log"

# Resources (for container providers)
[relaychain.nodes.resources]
request_memory = "1Gi"
request_cpu = "500m"
limit_memory = "2Gi"
limit_cpu = "1000m"
```

### Builder

```rust
.with_validator(|node| {
    node.with_name("alice")
        .with_command("polkadot")
        .with_image("parity/polkadot:latest")
        .validator(true)
        .invulnerable(true)
        .with_args(vec!["--alice".into(), "-lruntime=debug".into()])
        .with_env(vec![("RUST_LOG".into(), "info".into())])
        .with_resources(|r| {
            r.with_request_memory("1Gi").with_limit_memory("2Gi")
        })
})
```

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `name` | String | — | **Required.** Unique node name |
| `command` | String | Inherited | Binary command to execute |
| `subcommand` | String | — | Optional subcommand (e.g., for nested CLI) |
| `image` | String | Inherited | Container image |
| `args` | Array | — | CLI arguments |
| `env` | Array | — | Environment variables as `{name, value}` pairs |
| `validator` | Boolean | `true` | Whether node is a validator/authority |
| `invulnerable` | Boolean | `true` | Add to invulnerables set in genesis |
| `bootnode` | Boolean | `false` | Whether node acts as a bootnode |
| `initial_balance` | Number | 2000000000000 | Initial account balance (alias: `balance`) |
| `ws_port` | Number | Auto | WebSocket RPC port |
| `rpc_port` | Number | Auto | HTTP RPC port |
| `prometheus_port` | Number | Auto | Prometheus metrics port |
| `p2p_port` | Number | Auto | P2P networking port |
| `p2p_cert_hash` | String | — | libp2p WebRTC certificate hash |
| `bootnodes` | Array | — | Additional bootnode addresses |
| `db_snapshot` | String | — | Database snapshot path/URL |
| `keystore_path` | String | — | Custom keystore directory |
| `keystore_key_types` | Array | — | Key types to generate in keystore (e.g., `aura`, `gran`, `babe`) |
| `chain_spec_key_types` | Array | — | Session key types to inject into chain spec genesis |
| `override_eth_key` | String | — | Override auto-generated EVM session key |
| `log_path` | String | — | Path for node log file (alias: `node_log_path`) |
| `resources` | Object | — | Resource limits (see below) |

## Node Groups

Define multiple nodes with the same configuration. Creates `validator-0`, `validator-1`, etc.

### TOML

```toml
[[relaychain.node_groups]]
name = "validator"
count = 5
command = "polkadot"
```

### Builder

```rust
.with_relaychain(|r| {
    r.with_chain("rococo-local")
        .with_default_command("polkadot")
        .with_node_group(|g| {
            g.with_base_node(|node| {
                node.with_name("validator")
                    .with_args(vec!["-lruntime=debug".into()])
            })
            .with_count(5)
        })
})
```

## Resources

For container providers (Docker, Podman, Kubernetes).

```toml
[relaychain.default_resources]
request_memory = "1Gi"
request_cpu = "500m"
limit_memory = "2Gi"
limit_cpu = "1000m"
```

### Reference

| Option | Type | Description |
|--------|------|-------------|
| `request_memory` | String | Requested memory (e.g., `512Mi`, `1Gi`) |
| `request_cpu` | String | Requested CPU (e.g., `250m`, `1`) |
| `limit_memory` | String | Memory limit |
| `limit_cpu` | String | CPU limit |

