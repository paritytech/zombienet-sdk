# CLI Usage

The `zombie-cli` command-line tool spawns and manages ZombieNet networks.

## Installation

**Pre-built binaries:**

Download from [GitHub Releases](https://github.com/paritytech/zombienet-sdk/releases).

**From source:**

```bash
cargo install zombie-cli
```

---

## spawn

Spawn a network from a configuration file.

```bash
zombie-cli spawn <CONFIG> [OPTIONS]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `CONFIG` | Yes | Path to TOML configuration file |

### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--provider` | `-p` | `docker` | Provider: `native`, `docker`, or `k8s` |
| `--dir` | `-d` | Random temp | Base directory for network files |
| `--spawn-concurrency` | `-c` | 100 | Concurrent node spawn processes |
| `--node-verifier` | `-v` | `metric` | Readiness check: `metric` or `none` |

### Examples

```bash
# Spawn with native provider
zombie-cli spawn network.toml --provider native

# Spawn with custom directory
zombie-cli spawn network.toml --dir /tmp/my-network

# Spawn with Docker (default)
zombie-cli spawn network.toml

# Spawn on Kubernetes
zombie-cli spawn network.toml --provider k8s
```

## reproduce

Reproduce CI test runs locally in Docker. Downloads nextest archives from GitHub Actions and runs them in a containerized environment matching CI.

```bash
zombie-cli reproduce <REPO> <RUN_ID> [OPTIONS]
zombie-cli reproduce --archive <PATH> [OPTIONS]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `REPO` | Yes* | Repository name (e.g., `zombienet-sdk`) |
| `RUN_ID` | Yes* | GitHub Actions run ID (find in the URL of a workflow run) |

*Not required when using `--archive`.

### Options

| Option | Short | Description |
|--------|-------|-------------|
| `--archive` | `-a` | Path to local nextest archive (.tar.zst) |
| `--artifact-pattern` | `-p` | Filter artifact downloads by pattern (default: `*zombienet-artifacts*`) |
| `--skip` | `-s` | Skip tests matching pattern (repeatable) |
| `[TEST_FILTER]` | | Additional arguments passed to nextest (use `--` separator) |

### Requirements

- Docker installed and running
- GitHub CLI (`gh`) installed and authenticated (for downloading artifacts)
- `POLKADOT_SDK_PATH` environment variable pointing to your polkadot-sdk workspace

### How It Works

1. Downloads nextest test archives (`.tar.zst`) from GitHub Actions
2. Optionally downloads binary artifacts matching the pattern
3. Runs tests inside a Docker container with:
   - `ZOMBIE_PROVIDER=native` (native provider inside container)
   - `RUST_LOG=debug` for verbose output
   - Mounted polkadot-sdk workspace
4. Extracts binaries to `/tmp/binaries` inside the container

### Examples

```bash
# Reproduce from GitHub Actions (requires gh CLI authentication)
POLKADOT_SDK_PATH=/path/to/polkadot-sdk \
zombie-cli reproduce zombienet-sdk 123456789

# Reproduce from local archive (no gh CLI needed)
POLKADOT_SDK_PATH=/path/to/polkadot-sdk \
zombie-cli reproduce --archive ./my-archive.tar.zst

# Download only artifacts matching "ray" pattern
zombie-cli reproduce zombienet-sdk 123456789 -p ray

# Skip specific slow tests
zombie-cli reproduce zombienet-sdk 123456789 --skip "slow_test"

# Pass filters to nextest
zombie-cli reproduce zombienet-sdk 123456789 -- --test-filter my_test
```

### Nextest Archive Format

The `.tar.zst` archive is a [nextest](https://nexte.st/) archive containing compiled test binaries and metadata. These are produced by CI runs using `cargo nextest archive`.

---

## Environment Variables

| Variable | Command | Description |
|----------|---------|-------------|
| `RUST_LOG` | All | Logging level (e.g., `debug`, `info`) |
| `ZOMBIE_SPAWN_CONCURRENCY` | spawn | Override spawn concurrency |
| `POLKADOT_SDK_PATH` | reproduce | Path to polkadot-sdk workspace |
| `POLKADOT_IMAGE` | spawn | Override Polkadot container image |
| `CUMULUS_IMAGE` | spawn | Override Cumulus container image |

---
