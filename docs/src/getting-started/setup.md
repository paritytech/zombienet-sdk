# Setup

### Prerequisites

- **Rust** - Install via [rustup](https://rustup.rs/)
- **A provider** (at least one):
  - **Native**: Binaries used by your network in PATH (or absolute paths in config), e.g., `polkadot`, `polkadot-parachain`
  - **Docker/Podman**: Container runtime installed and running
  - **Kubernetes**: Cluster access with `kubectl` configured

### Installation

#### CLI

Download pre-built binaries from [GitHub Releases](https://github.com/paritytech/zombienet-sdk/releases), or install via Cargo:

```bash
cargo install zombie-cli
```

Or build from source:

```bash
git clone https://github.com/paritytech/zombienet-sdk
cd zombienet-sdk
cargo build --release -p zombie-cli
# Binary will be at target/release/zombie-cli
```

#### As a Dependency

Add to your `Cargo.toml`:

```toml
[dependencies]
zombienet-sdk = "0.4"
```
