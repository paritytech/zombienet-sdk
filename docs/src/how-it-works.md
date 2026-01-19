# How It Works

This document covers the two core aspects of what ZombieNet does under the hood: **chain spec customization** and **command-line argument generation**.

## Chain Spec Customization

When spawning a network, ZombieNet modifies chain specifications to configure the test environment. These customizations happen automatically based on your network configuration.

### Relay Chain Customizations

The framework applies these modifications to relay chain specs:

| Customization | Description |
|---------------|-------------|
| **Runtime Genesis Patch** | Merges user-provided `genesis_overrides` JSON (applied first) |
| **Clear Authorities** | Removes existing session/grandpa/aura authorities |
| **Add Balances** | Adds balances for each node based on staking minimum |
| **Add Zombie Account** | Adds `//Zombie` account with 1000 tokens for internal operations |
| **Add Staking** | Configures staking for validators with minimum stake |
| **Add Session Authorities** | Adds validators as session authorities (if `session` pallet exists) |
| **Add Aura/Grandpa Authorities** | For chains without session pallet |
| **Add HRMP Channels** | Configures cross-chain messaging channels |
| **Register Parachains** | Adds `InGenesis` parachains to genesis |

### Parachain Customizations

For cumulus-based parachains:

| Customization | Description |
|---------------|-------------|
| **Override para_id/paraId** | Sets correct parachain ID in chain spec root |
| **Override relay_chain** | Sets relay chain ID in chain spec |
| **Apply Genesis Overrides** | Merges user-provided JSON overrides |
| **Clear/Add Authorities** | Configures collator session or aura authorities |
| **Add Collator Selection** | Adds invulnerable collators to `collatorSelection` |
| **Override parachainInfo** | Sets correct `para_id` in runtime genesis |
| **Add Balances** | Initial balances from assets pallet config |

For EVM-based parachains (`is_evm_based = true`), Ethereum session keys are generated and used instead of standard keys.

### Optional Overrides

Two additional overrides can be applied after the raw chain spec is generated:

- **WASM Override** (`wasm_override`): Replaces runtime code (`:code` storage key) with custom WASM
- **Raw Spec Override** (`raw_spec_override`): Applies JSON patch merge to override any part of the raw spec

---

## Command-Line Arguments

ZombieNet generates command-line arguments for each spawned node. Understanding what gets added helps debug issues and customize node behavior.

### Framework-Managed Arguments

These arguments are **always set by the framework** and filtered out if provided by user configuration:

| Argument | Value |
|----------|-------|
| `--chain` | Generated chain spec path |
| `--name` | Node name from config |
| `--rpc-cors` | `all` |
| `--rpc-methods` | `unsafe` |
| `--parachain-id` | Parachain ID (for parachain nodes) |
| `--node-key` | Deterministic key derived from node name |

Additionally, these flags are managed by the framework:

| Flag | When Added |
|------|------------|
| `--no-telemetry` | Always (relay chain nodes) |
| `--collator` | Cumulus collators with `is_validator: true` |

### Arguments Added Conditionally

| Argument | Condition |
|----------|-----------|
| `--validator` | Relay chain nodes with `is_validator: true` |
| `--insecure-validator-i-know-what-i-do` | Validators, if the binary supports it |
| `--prometheus-external` | Unless already in user args |
| `--unsafe-rpc-external` | Only for Docker/Kubernetes providers (not native) |
| `--bootnodes` | When bootnode addresses are available |

### Port Arguments

The framework injects port arguments based on assigned ports:

| Argument | Description |
|----------|-------------|
| `--prometheus-port` | Prometheus metrics port |
| `--rpc-port` | RPC/WebSocket port |
| `--listen-addr` | P2P listen address (format: `/ip4/0.0.0.0/tcp/{port}/ws`) |
| `--base-path` | Data directory path |

### Cumulus Collator Arguments

Cumulus-based collators receive two sets of arguments separated by `--`:

**Before `--`** (collator arguments):
- `--chain` (parachain spec)
- `--collator`
- `--prometheus-port`, `--rpc-port`, `--listen-addr`
- `--base-path`
- User-provided collator args

**After `--`** (embedded relay chain full node):
- `--chain` (relay chain spec)
- `--base-path` (separate directory)
- `--execution wasm`
- `--port` (full node P2P port)
- `--prometheus-port` (full node metrics port)
- User-provided full node args

### Removing Framework Arguments

Use the `-:` prefix to remove arguments that the framework adds:

```toml
[[relaychain.nodes]]
name = "bob"
args = ["-:--insecure-validator-i-know-what-i-do"]
```

This removes `--insecure-validator-i-know-what-i-do` from bob's command line.

The removal syntax:
- `-:--flag-name` removes `--flag-name`
- `-:flag-name` also removes `--flag-name` (normalized automatically)
- Works for both flags and options (the option's value is also removed)

### Example: Generated Relay Chain Command

```bash
polkadot \
  --chain /cfg/rococo-local.json \
  --name alice \
  --rpc-cors all \
  --rpc-methods unsafe \
  --unsafe-rpc-external \          # Only for Docker/K8s
  --node-key <deterministic_key> \
  --no-telemetry \
  --prometheus-external \
  --validator \
  --insecure-validator-i-know-what-i-do \
  --prometheus-port 9615 \
  --rpc-port 9944 \
  --listen-addr /ip4/0.0.0.0/tcp/30333/ws \
  --base-path /data \
  --bootnodes <bootnode_multiaddrs> \
  [user_args...]
```

### Example: Generated Cumulus Collator Command

```bash
polkadot-parachain \
  --chain /cfg/1000.json \
  --name collator01 \
  --rpc-cors all \
  --rpc-methods unsafe \
  --unsafe-rpc-external \          
  --node-key <deterministic_key> \
  --prometheus-external \
  --collator \
  --prometheus-port 9615 \
  --rpc-port 9944 \
  --listen-addr /ip4/0.0.0.0/tcp/30333/ws \
  --base-path /data \
  --bootnodes <para_bootnode_multiaddrs> \
  [user_collator_args...] \
  -- \
  --base-path /relay-data \
  --chain /cfg/rococo-local.json \
  --execution wasm \
  --port 30334 \
  --prometheus-port 9616 \
  [user_full_node_args...]
```
