# Introduction

ZombieNet SDK is a Rust testing framework for [Polkadot SDK](https://github.com/paritytech/polkadot-sdk)-based blockchains. It spawns ephemeral blockchain networks programmatically for testing and development.

The SDK succeeds the original [ZombieNet](https://github.com/paritytech/zombienet) (TypeScript), offering a type-safe, composable Rust API.

> **Migrating from TypeScript ZombieNet?** The TOML configuration format is largely compatible. See [Network Definition Spec](./network-spec/) for the full reference.

## Key Features

- **Programmatic Network Spawning** - Define and spawn relay chains and parachains from Rust code
- **Fluent Builder API** - Compose networks using a chainable builder pattern
- **Multiple Providers** - Run on Kubernetes, Podman, Docker, or natively
- **Metrics & Assertions** - Query Prometheus metrics and validate network behavior
- **Subxt Integration** - Interact with networks using [subxt](https://github.com/paritytech/subxt)

## Use Cases

- **Integration Testing** - Test cross-chain functionality with complete networks
- **CI/CD Pipelines** - Automate network deployment and testing
- **Development** - Spin up local networks for development and debugging
- **Parachain Testing** - Test registration, block production etc.

## Next

See [Getting Started](./getting-started/) to spawn your first network.
