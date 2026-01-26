# Environment Variables

ZombieNet SDK uses environment variables for configuration overrides, container images, and CI integration.

## Container Images

Override default images for Docker and Kubernetes providers:

| Variable | Default | Description |
|----------|---------|-------------|
| `POLKADOT_IMAGE` | `docker.io/parity/polkadot:latest` | Relay chain node image |
| `CUMULUS_IMAGE` | `docker.io/parity/polkadot-parachain:latest` | Parachain collator image |
| `MALUS_IMAGE` | `docker.io/paritypr/malus:latest` | Malus (malicious node) image |
| `COL_IMAGE` | `docker.io/paritypr/colander:latest` | Colander image |

Example:
```bash
POLKADOT_IMAGE=docker.io/parity/polkadot:v1.5.0 zombie-cli spawn network.toml
```

---

## Provider Selection

| Variable | Values | Default | Description |
|----------|--------|---------|-------------|
| `ZOMBIE_PROVIDER` | `native`, `k8s`, `docker` | `docker` | Default provider when not specified |

Example:
```bash
ZOMBIE_PROVIDER=native zombie-cli spawn network.toml
```

---

## Spawn Control

| Variable | Description |
|----------|-------------|
| `ZOMBIE_SPAWN_CONCURRENCY` | Override spawn concurrency (default: 100). Set to lower values for resource-constrained environments. |
| `ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS` | Override per-node spawn timeout in seconds |

Example:
```bash
ZOMBIE_SPAWN_CONCURRENCY=10 zombie-cli spawn network.toml
```

---

## CLI-Specific

| Variable | Command | Description |
|----------|---------|-------------|
| `POLKADOT_SDK_PATH` | `reproduce` | Path to local polkadot-sdk workspace (required for reproduce) |
| `RUST_LOG` | All | Logging level (`debug`, `info`, `warn`, `error`) |

---

## Other

| Variable | Description |
|----------|-------------|
| `ZOMBIE_RM_TGZ_AFTER_EXTRACT` | Remove tgz archives after extraction (used for db snapshot extraction) |

---

## TOML Template Replacement

Environment variables can be referenced in TOML configuration files using `{{VAR_NAME}}` syntax:

```toml
[relaychain]
default_image = "{{POLKADOT_IMAGE}}"

[[relaychain.nodes]]
name = "alice"
image = "{{CUSTOM_NODE_IMAGE}}"
```

Any environment variable can be substituted. If the variable is not set, the placeholder remains unchanged.

---

## Runtime Token Replacement

For dynamic values from running nodes, use `{{ZOMBIE:node_name:field}}` syntax in node arguments:

```toml
[[relaychain.nodes]]
name = "bob"
args = ["--sync-target", "{{ZOMBIE:alice:multiaddr}}"]
```

Available fields:

| Field | Aliases | Description |
|-------|---------|-------------|
| `multiaddr` | `multiAddress` | Node's libp2p multiaddress |
| `ws_uri` | `wsUri` | WebSocket RPC endpoint |
| `prometheus_uri` | `prometheusUri` | Prometheus metrics endpoint |

When using token replacement, spawn concurrency is automatically set to 1 (serial) to ensure dependencies are resolved correctly.
