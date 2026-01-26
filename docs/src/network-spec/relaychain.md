# Relaychain

### TOML

```toml
[relaychain]
chain = "rococo-local"
default_command = "polkadot"
default_image = "parity/polkadot:latest"
default_args = ["-lruntime=debug"]

# Optional: Use a pre-existing chain spec
# chain_spec_path = "/path/to/chain-spec.json"

# Optional: Generate chain spec with a command
# chain_spec_command = "polkadot build-spec --chain {chain}"

# Optional: Runtime for chain spec generation
# chain_spec_runtime.src = "/path/to/runtime.wasm"
# chain_spec_runtime.preset = "development"

# Genesis configuration
random_nominators_count = 4
max_nominations = 16

# Genesis patch (applied to chain spec)
[relaychain.genesis.balances]
balances = [["5GrwvaEF...", 1000000000000]]

[[relaychain.nodes]]
name = "alice"
invulnerable = true

[[relaychain.nodes]]
name = "bob"
args = ["-lparachain=trace"]
```

### Builder

```rust
use zombienet_sdk::NetworkConfigBuilder;

let config = NetworkConfigBuilder::new()
    .with_relaychain(|r| {
        r.with_chain("rococo-local")
            .with_default_command("polkadot")
            .with_default_image("parity/polkadot:latest")
            .with_validator(|v| v.with_name("alice").invulnerable(true))
            .with_validator(|v| v.with_name("bob"))
    })
    .build()
    .unwrap();
```

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `chain` | String | `rococo-local` | Chain name (e.g., `polkadot`, `rococo-local`, `westend`) |
| `default_command` | String | `polkadot` | Default binary command for nodes |
| `default_image` | String | — | Default container image (Docker/Podman/K8s) |
| `default_args` | Array | — | Default CLI arguments for all nodes |
| `default_resources` | Object | — | Default resource limits (see [Resources](./node.md#resources)) |
| `default_db_snapshot` | String | — | Default database snapshot path/URL |
| `chain_spec_path` | String | — | Path or URL to pre-existing chain spec |
| `chain_spec_command` | String | — | Command template to generate chain spec |
| `chain_spec_command_is_local` | Boolean | `false` | Run chain spec command locally |
| `chain_spec_command_output_path` | String | `/dev/stdout` | Output path for chain spec command |
| `chain_spec_runtime` | Object | — | Runtime WASM for chain spec generation (see [Chain Spec](./chain-spec.md)) |
| `random_nominators_count` | Number | — | Number of random nominators for staking |
| `max_nominations` | Number | — | Maximum nominations per nominator |
| `genesis` | Object | — | Genesis overrides as JSON (alias: `runtime_genesis_patch`) |
| `wasm_override` | String | — | WASM runtime override path/URL |
| `raw_spec_override` | String/Object | — | Raw chain spec override (inline JSON or file path) |
