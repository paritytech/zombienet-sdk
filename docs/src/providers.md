# Providers

Providers determine how ZombieNet spawns and manages nodes. Choose based on your environment and requirements.

## Quick Comparison

| Provider | Runs As | Image Required | Resource Limits |
|----------|---------|----------------|-----------------|
| Native | Local processes | No | No |
| Docker | Containers | Yes | No |
| Kubernetes | K8s pods | Yes | Yes |

## Provider Selection

Set the default provider via environment variable:

```bash
export ZOMBIE_PROVIDER=native  # or: docker, k8s
```

Or specify per-command with `--provider` flag. Default is `docker`.

---

## Native

Runs nodes directly on your machine as local processes.

### Requirements

- Node binaries in PATH (e.g., `polkadot`, `polkadot-parachain`)
- Or specify absolute paths in configuration

### Usage

**CLI:**
```bash
zombie-cli spawn network.toml --provider native
```

**Programmatically:**
```rust
let network = config.spawn_native().await?;
```

### Notes

- Fastest startup, no container overhead
- No container images needed
- Ports are assigned randomly to avoid conflicts
- Files stored in a temp directory (or `base_dir` if configured)

---

## Docker

Runs nodes in Docker containers.

### Requirements

- Docker or Podman installed and running
- Container images with node binaries

### Usage

**CLI:**
```bash
zombie-cli spawn network.toml --provider docker
```

**Programmatically:**
```rust
let network = config.spawn_docker().await?;
```

### Default Images

Override via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `POLKADOT_IMAGE` | `docker.io/parity/polkadot:latest` | Relay chain nodes |
| `CUMULUS_IMAGE` | `docker.io/parity/polkadot-parachain:latest` | Parachain collators |
| `MALUS_IMAGE` | `docker.io/paritypr/malus:latest` | Malus (malicious node) |
| `COL_IMAGE` | `docker.io/paritypr/colander:latest` | Colander |

## Kubernetes

Deploys nodes as Kubernetes pods.

### Requirements

- Kubernetes cluster access
- `kubectl` configured with valid kubeconfig
- Container images accessible to the cluster

### Usage

**CLI:**
```bash
zombie-cli spawn network.toml --provider k8s
```

**Programmatically:**
```rust
let network = config.spawn_k8s().await?;
```

### Resource Limits

Kubernetes is the only provider supporting resource requests and limits:

```toml
[relaychain.default_resources]
request_memory = "512Mi"
request_cpu = "250m"
limit_memory = "1Gi"
limit_cpu = "500m"
```

## Attaching to Running Networks

Reconnect to a previously spawned network, currently running network using the `zombie.json` state file:

```rust
use zombienet_sdk::{AttachToLive, AttachToLiveNetwork};

// Native
let network = AttachToLiveNetwork::attach_native("/tmp/zombie-1/zombie.json".into()).await?;

// Docker
let network = AttachToLiveNetwork::attach_docker("/tmp/zombie-1/zombie.json".into()).await?;

// Kubernetes
let network = AttachToLiveNetwork::attach_k8s("/tmp/zombie-1/zombie.json".into()).await?;
```

The `zombie.json` file is written to the network's base directory after spawn completes.
