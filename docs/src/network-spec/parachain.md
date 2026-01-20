# Parachain

### TOML

```toml
[[parachains]]
id = 1000
chain = "asset-hub-rococo-local"
cumulus_based = true
onboard_as_parachain = true
default_command = "polkadot-parachain"
balance = 2000000000000

# Registration strategy (pick one):
# add_to_genesis = true    # InGenesis (default, fastest)
# register_para = true     # UsingExtrinsic (via extrinsic after spawn)
# (neither = Manual, no automatic registration)

# Genesis configuration
# genesis_wasm_path = "/path/to/para-wasm"
# genesis_state_path = "/path/to/para-genesis-state"

# Or generate genesis artifacts
# genesis_wasm_generator = "polkadot-parachain export-genesis-wasm"
# genesis_state_generator = ["polkadot-parachain", "export-genesis-state"]

# Genesis patch
[parachains.genesis.balances]
balances = [["5GrwvaEF...", 1000000000000]]

[[parachains.collators]]
name = "collator01"
command = "polkadot-parachain"
validator = true

[[parachains.collators]]
name = "collator02"
args = ["-lparachain=trace"]
```

### Builder

```rust
use zombienet_sdk::{NetworkConfigBuilder, RegistrationStrategy};

let config = NetworkConfigBuilder::new()
    .with_relaychain(|r| {
        r.with_chain("rococo-local")
            .with_default_command("polkadot")
            .with_validator(|v| v.with_name("alice"))
            .with_validator(|v| v.with_name("bob"))
    })
    .with_parachain(|p| {
        p.with_id(1000)
            .cumulus_based(true)
            .with_collator(|c| c.with_name("collator01").with_command("polkadot-parachain"))
    })
    .build()
    .unwrap();
```

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `id` | Number | — | **Required.** Unique parachain ID |
| `chain` | String | — | Chain name for the parachain |
| `cumulus_based` | Boolean | `true` | Whether parachain uses Cumulus |
| `evm_based` | Boolean | `false` | Whether parachain is EVM-based (e.g., Frontier) |
| `onboard_as_parachain` | Boolean | `true` | Register as parachain (vs parathread) |
| `add_to_genesis` | Boolean | — | Register in genesis (InGenesis strategy). Mutually exclusive with `register_para` |
| `register_para` | Boolean | — | Register via extrinsic (UsingExtrinsic strategy). Mutually exclusive with `add_to_genesis` |
| `balance` | Number | 2000000000000 | Initial balance for parachain account |
| `default_command` | String | — | Default collator command |
| `default_image` | String | — | Default container image |
| `default_args` | Array | — | Default CLI arguments |
| `default_resources` | Object | — | Default resource limits (see [Resources](./node.md#resources)) |
| `default_db_snapshot` | String | — | Default database snapshot path/URL |
| `genesis_wasm_path` | String | — | Path/URL to genesis WASM blob |
| `genesis_wasm_generator` | String | — | Command to generate genesis WASM |
| `genesis_state_path` | String | — | Path/URL to genesis state |
| `genesis_state_generator` | String/Array | — | Command with args to generate genesis state |
| `genesis` | Object | — | Genesis overrides as JSON (alias: `runtime_genesis_patch`) |
| `chain_spec_path` | String | — | Path/URL to parachain chain spec |
| `chain_spec_command` | String | — | Command template to generate chain spec |
| `chain_spec_command_is_local` | Boolean | `false` | Run chain spec command locally |
| `chain_spec_command_output_path` | String | `/dev/stdout` | Output path for chain spec command |
| `chain_spec_runtime` | Object | — | Runtime WASM for chain spec generation |
| `wasm_override` | String | — | WASM runtime override path/URL |
| `raw_spec_override` | String/Object | — | Raw chain spec override (inline JSON or file path) |
| `bootnodes_addresses` | Array | — | Bootnode multiaddresses |
| `no_default_bootnodes` | Boolean | `false` | Don't auto-assign bootnode role if none specified |

## Registration Strategies

- **InGenesis** - In relay chain genesis (fastest)
- **UsingExtrinsic** - Via extrinsic after spawn
- **Manual** - No automatic registration

## Collator Groups

```toml
[[parachains.collator_groups]]
name = "collator"
count = 3
command = "polkadot-parachain"
```

```rust
.with_parachain(|p| {
    p.with_id(1000)
        .with_collator_group(|g| {
            g.with_count(3)
                .with_base_node(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
})
```
