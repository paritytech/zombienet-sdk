# Examples

All examples are in [`crates/examples/`](https://github.com/paritytech/zombienet-sdk/tree/main/crates/examples).

Run Rust examples with `cargo run --example <example_name>`.

Spawn from config files with `zombie-cli spawn -p <provider> <config-file>`.

---

## Basic Network Setup

Start here to understand network configuration patterns.

| Example | Description |
|---------|-------------|
| [`simple_network_example`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/simple_network_example.rs) | Minimal relay chain with two validators using TOML config |
| [`small_network_with_default`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/small_network_with_default.rs) | Override default command and image for all nodes |
| [`small_network_with_base_dir`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/small_network_with_base_dir.rs) | Custom base directory for node data |
| [`small_network_config`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/small_network_config.rs) | Minimal programmatic configuration |

---

## Parachain Lifecycle

Configure, register, and manage parachains.

| Example | Description |
|---------|-------------|
| [`small_network_with_para`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/small_network_with_para.rs) | Basic relay + parachain topology |
| [`register_para`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/register_para.rs) | Register parachain via extrinsic |
| [`add_para`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/add_para.rs) | Add parachain to running network |
| [`para_upgrade`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/para_upgrade.rs) | Runtime upgrade on running parachain |
| [`two_paras_same_id`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/two_paras_same_id.rs) | Handling duplicate parachain IDs |

---

## Node Groups

Scale networks with grouped nodes.

| Example | Description |
|---------|-------------|
| [`big_network_with_group_nodes`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/big_network_with_group_nodes.rs) | Programmatic network with grouped nodes |
| [`network_example_with_group_nodes`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/network_example_with_group_nodes.rs) | TOML config with group nodes |

---

## Advanced Configuration

| Example | Description |
|---------|-------------|
| [`resource_limits`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/resource_limits.rs) | CPU and memory limits for containers |
| [`wasm-override`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/wasm-override.rs) | Custom WASM runtime override |
| [`arg_removal`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/arg_removal.rs) | Remove default CLI arguments |
| [`db_snapshot`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/db_snapshot.rs) | Database snapshots for faster init |
| [`docker_db_snapshot`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/docker_db_snapshot.rs) | DB snapshots in Docker environments |
| [`raw_spec_override`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/raw_spec_override.rs) | Override raw chain spec fields |

---

## Chain Spec Generation

| Example | Description |
|---------|-------------|
| [`chain_spec_generation`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/chain_spec_generation.rs) | Dynamic chain spec generation |
| [`chain_spec_runtime_kusama`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/chain_spec_runtime_kusama.rs) | Kusama with custom runtime WASM |
| [`polkadot_people_wasm_runtime`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/polkadot_people_wasm_runtime.rs) | Polkadot + People parachain with custom runtimes |
| [`genesis_state_generator_example`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/genesis_state_generator_example.rs) | Custom genesis state generator |

---

## Keys and Security

| Example | Description |
|---------|-------------|
| [`keystore_key_types`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/keystore_key_types.rs) | Keystore directories and key type validation |
| [`chain_spec_key_types`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/chain_spec_key_types.rs) | Chain spec session key configuration |
| [`evm_parachain_session_key`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/evm_parachain_session_key.rs) | EVM parachain session keys |

---

## Network Utilities

| Example | Description |
|---------|-------------|
| [`from_live`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/from_live.rs) | Attach to running network via `zombie.json` |
| [`test_run_script`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/test_run_script.rs) | Run scripts on nodes |

---

## Config Files

Example TOML configs are in [`crates/examples/examples/configs/`](https://github.com/paritytech/zombienet-sdk/tree/main/crates/examples/examples/configs):

| Config | Description |
|--------|-------------|
| [`simple.toml`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/configs/simple.toml) | Basic two-validator relay chain |
| [`simple-group-nodes.toml`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/configs/simple-group-nodes.toml) | Relay chain with node groups |
| [`resource_limits.toml`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/configs/resource_limits.toml) | Container resource limits |
| [`wasm-override.toml`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/configs/wasm-override.toml) | WASM runtime override |
| [`arg-removal.toml`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/configs/arg-removal.toml) | CLI argument removal |
| [`polkadot-ah-chain-spec-runtime.toml`](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/configs/polkadot-ah-chain-spec-runtime.toml) | Polkadot + Asset Hub with custom runtimes |
