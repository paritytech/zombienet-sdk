# How It Works

This document covers the two core aspects of what ZombieNet does under the hood: **chain spec customization** and **command-line argument generation**.

## Chain Spec Customization

When spawning a network, ZombieNet modifies chain specifications to configure the test environment. These customizations happen automatically based on your network configuration.

### Relay Chain Customizations

The framework applies these modifications to relay chain specs:

| Customization | Description |
|---------------|-------------|
| **Runtime Genesis Patch** | Merges user-provided `genesis_overrides` JSON (applied first) |
| **Clear Authorities** | Removes existing authorities from `session/keys`, `aura/authorities`, `grandpa/authorities`, `collatorSelection/invulnerables`, and `staking/invulnerables` + `staking/stakers` (relay only). Also sets `validatorCount` to 0 unless `devStakers` is configured. |
| **Add Balances** | Adds balances for each node's `sr` and `sr_stash` accounts. Balance is `max(initial_balance, staking_min * 2)`. Only nodes with `initial_balance > 0` receive balances. |
| **Add Zombie Account** | Adds `//Zombie` account with 1000 units (in chain denomination, i.e., `1000 * 10^token_decimals` planck) for internal operations |
| **Add Staking** | Configures staking for validators with minimum stake |
| **Add Session Authorities** | Adds validators as session authorities (if `session` pallet exists) |
| **Add Aura/Grandpa Authorities** | For chains without session pallet |
| **Add HRMP Channels** | Configures cross-chain messaging channels |
| **Register Parachains** | Adds `InGenesis` parachains to genesis |

### Parachain Customizations

For cumulus-based parachains:

| Customization | Description |
|---------------|-------------|
| **Override para_id/paraId** | Sets correct parachain ID in chain spec root (both `para_id` and `paraId` variants) |
| **Override relay_chain** | Sets relay chain ID in chain spec root |
| **Apply Genesis Overrides** | Merges user-provided JSON overrides |
| **Clear/Add Authorities** | Clears existing authorities from `session/keys`, `aura/authorities`, `grandpa/authorities`, `collatorSelection/invulnerables`. Then adds collator session keys (if session pallet exists) or aura authorities. |
| **Add Collator Selection** | Adds invulnerable collators to `collatorSelection/invulnerables` |
| **Override parachainInfo** | Sets correct `parachainId` in `/parachainInfo/parachainId` |
| **Add Balances** | Extracts accounts from assets pallet metadata and ensures they have native token balances. Adds balances for collator `sr` accounts. |

For EVM-based parachains (`is_evm_based = true`), Ethereum session keys are generated and used instead of standard keys.

### Chain Spec Key Types

ZombieNet supports customizing session key types and their cryptographic schemes via the `chain_spec_key_types` configuration option.

#### Cryptographic Schemes

| Scheme | Suffix | Description |
|--------|--------|-------------|
| **SR25519** | `_sr` | Schnorr signatures, used by most runtime keys |
| **ED25519** | `_ed` | Edwards curve signatures, used by grandpa |
| **ECDSA** | `_ec` | Elliptic curve, used by beefy and Ethereum compatibility |

#### Predefined Key Types

These key types have predefined default schemes:

| Key Type | Default Scheme | Notes |
|----------|----------------|-------|
| `aura` | SR25519 | ED25519 on asset-hub-polkadot |
| `babe` | SR25519 | |
| `grandpa` | ED25519 | |
| `beefy` | ECDSA | |
| `im_online` | SR25519 | |
| `authority_discovery` | SR25519 | |
| `para_validator` | SR25519 | |
| `para_assignment` | SR25519 | |
| `parachain_validator` | SR25519 | |
| `nimbus` | SR25519 | |
| `vrf` | SR25519 | |

#### Syntax

Two formats are supported:

- **Short form**: `aura` — uses the predefined default scheme
- **Long form**: `aura_ed` — explicitly specifies the scheme

Unknown key types default to SR25519 when using short form.

#### Example

```toml
[relaychain]
chain_spec_key_types = ["aura", "grandpa", "beefy"]  # Uses defaults

# Or with explicit schemes:
chain_spec_key_types = ["aura_ed", "grandpa_sr", "custom_key_ec"]
```

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
| `--parachain-id` | Parachain ID (relay chain nodes only, when `para_id` is specified; NOT used for cumulus collators) |
| `--node-key` | Deterministic key derived from node name |

Additionally, these flags are managed by the framework:

| Flag | When Added |
|------|------------|
| `--no-telemetry` | Relay chain nodes only (not cumulus collators) |
| `--collator` | Cumulus collators with `is_validator: true` |
| `--execution wasm` | Embedded relay chain full node (after `--` separator) |

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
- `--base-path` (separate relay data directory)
- `--chain` (relay chain spec)
- `--execution wasm` (hardcoded, always WASM execution)
- `--port` (full node P2P port, injected by framework when assigned port differs from default 30333)
- `--prometheus-port` (full node metrics port, injected by framework when assigned port differs from default 9616)
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
  --unsafe-rpc-external \          # omitted for native provider
  --node-key <deterministic_key> \
  --prometheus-external \
  --validator \
  --insecure-validator-i-know-what-i-do \  # if binary supports it
  --bootnodes <bootnode_multiaddrs> \
  --prometheus-port 9615 \
  --rpc-port 9944 \
  --listen-addr /ip4/0.0.0.0/tcp/30333/ws \
  --base-path /data \
  --no-telemetry \
  [user_args...]
```

### Example: Generated Cumulus Collator Command

```bash
polkadot-parachain \
  --chain /cfg/1000.json \
  --name collator01 \
  --rpc-cors all \
  --rpc-methods unsafe \
  --unsafe-rpc-external \          # omitted for native provider
  --node-key <deterministic_key> \
  --prometheus-external \
  --collator \                     # if is_validator: true
  --bootnodes <para_bootnode_multiaddrs> \
  --prometheus-port 9615 \
  --rpc-port 9944 \
  --listen-addr /ip4/0.0.0.0/tcp/30333/ws \
  --base-path /data \
  [user_collator_args...] \
  -- \
  --base-path /relay-data \
  --chain /cfg/rococo-local.json \
  --execution wasm \
  --port 30334 \                   # if assigned port differs from default
  --prometheus-port 9616 \         # if assigned port differs from default
  [user_full_node_args...]
```
