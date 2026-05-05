# zombienet-sdk Rust builder API reference

Source of truth: `crates/configuration/src/` (network.rs, relaychain.rs, parachain.rs, node.rs, hrmp_channel.rs, global_settings.rs) and `crates/sdk/src/lib.rs` for re-exports. When uncertain about a method's exact name or signature, grep there — this file is a guide, not a spec.

## Imports

```rust
use zombienet_sdk::{
    NetworkConfig,
    NetworkConfigBuilder,
    NetworkConfigExt,        // .spawn_native() / .spawn_k8s() / .spawn_docker()
    AddNodeOptions,
    AddCollatorOptions,
    LocalFileSystem,
    subxt,
    subxt_signer,
};
```

## NetworkConfigBuilder — top level

```rust
NetworkConfigBuilder::new()
    .with_relaychain(|r| { ... })   // REQUIRED, exactly once
    .with_parachain(|p| { ... })    // 0+ times
    .with_hrmp_channel(|c| { ... }) // 0+ times
    .with_global_settings(|g| { ... })
    .build()                        // -> Result<NetworkConfig, Vec<...>>
```

The builder is typestate: `NetworkConfigBuilder<Initial>` becomes `NetworkConfigBuilder<WithRelaychain>` after `.with_relaychain(...)`. You cannot call `.build()` without a relaychain.

`.build()` returns `Result<NetworkConfig, Vec<...>>` — the error is a **vector** of validation errors. Convert to a single error before `?`:

```rust
.build()
.map_err(|errs| anyhow::anyhow!("config invalid: {errs:?}"))?
```

You can also load from TOML:

```rust
let cfg = NetworkConfig::load_from_toml("path/to/config.toml")?;
```

`load_from_toml` returns the same type the builder produces, so everything below applies to both.

## RelaychainConfigBuilder

Inside `.with_relaychain(|r| ...)`:

| Method | Purpose |
|---|---|
| `.with_chain(name)` | `"rococo-local"`, `"westend-local"`, `"paseo-local"`, etc., or path to chain spec. |
| `.with_default_command(cmd)` | Default binary for every node (e.g., `"polkadot"`). |
| `.with_default_image(img)` | Container image for k8s/docker. |
| `.with_default_args(args)` | CLI args appended to every node. |
| `.with_default_resources(\|r\| ...)` | k8s resource defaults. |
| `.with_chain_spec_path(p)` | Pre-built chain spec. |
| `.with_chain_spec_command(cmd)` | Command to generate the chain spec. |
| `.with_runtime_genesis_patch(json)` | JSON patch over genesis. |
| `.with_random_nominators_count(n)` | Auto-create N nominators. |
| `.with_max_nominations(n)` | Per-account nomination cap. |
| `.with_node(\|n\| ...)` | Add a named node. |
| `.with_node_group(\|g\| ...)` | Add a group of identical nodes. |
| `.with_override_session_0(bool)` | Override session 0 to allow paras to produce blocks immediately (see commit `5b46fff` for context). |

Closures return the builder so they chain; the outer `.with_relaychain(...)` also returns the chained builder.

## ParachainConfigBuilder

Inside `.with_parachain(|p| ...)`:

| Method | Purpose |
|---|---|
| `.with_id(n)` | Para ID. REQUIRED. |
| `.cumulus_based(bool)` | `true` for modern parachains. |
| `.with_chain(name)` | Optional chain spec name. |
| `.with_default_command(cmd)` | `"polkadot-parachain"` typically. |
| `.with_default_image(img)` | Container image. |
| `.with_default_args(args)` | CLI args appended to every collator. |
| `.with_collator(\|c\| ...)` | Add a named collator. |
| `.with_collator_group(\|g\| ...)` | Add a group of collators. |
| `.with_genesis_wasm_path(p)` | Pre-built genesis wasm. |
| `.with_genesis_wasm_generator(cmd)` | Command to generate the wasm. |
| `.with_genesis_state_path(p)` | Pre-built genesis state. |
| `.with_genesis_state_generator(cmd)` | Command to generate the state. |
| `.with_wasm_override(p)` | Live runtime upgrade testing. |
| `.with_chain_spec_path(p)` | Pre-built chain spec. |
| `.with_runtime_genesis_patch(json)` | JSON patch over para genesis. |
| `.onboard_as_parachain(bool)` | Onboard immediately (vs parathread). |
| `.add_to_genesis(bool)` | Register in genesis (vs runtime extrinsic). |
| `.register_para(bool)` | Register via extrinsic at runtime. |

## NodeConfigBuilder (relay nodes AND collators)

The same builder shape is used for both — `.with_validator(...)` / `.with_node(...)` for relay, `.with_collator(...)` for paras.

| Method | Purpose |
|---|---|
| `.with_name(s)` | REQUIRED. Unique. |
| `.with_command(cmd)` | Override default command. |
| `.with_image(img)` | Override default image. |
| `.with_args(args)` | Per-node CLI args. |
| `.validator(bool)` | Default true for relay, false for paras. |
| `.invulnerable(bool)` | Add to invulnerable set. |
| `.bootnode(bool)` | Make this node a bootnode. |
| `.with_balance(n)` | Initial balance. |
| `.with_rpc_port(n)` | Pin the RPC port (don't use in parallel tests). |
| `.with_prometheus_port(n)` | Pin metrics port. |
| `.with_p2p_port(n)` | Pin libp2p port. |
| `.with_log_path(p)` | Stream logs to a specific file. |
| `.with_db_snapshot(p)` | Path/URL to seed DB. |
| `.with_env(vec)` | `vec![("KEY", "value")]` style. |
| `.with_resources(\|r\| ...)` | k8s resources. |
| `.with_subcommand(s)` | Subcommand to invoke (advanced). |

## HrmpChannelConfigBuilder

```rust
.with_hrmp_channel(|c| {
    c.with_sender(100)
        .with_recipient(101)
        .with_max_capacity(8)
        .with_max_message_size(512)
})
```

## GlobalSettingsBuilder

```rust
.with_global_settings(|g| {
    g.with_network_spawn_timeout(1200)   // seconds
        .with_node_spawn_timeout(120)
        .with_local_ip("127.0.0.1")
        .with_base_dir("/tmp/zn-test")
})
```

## Spawning — NetworkConfigExt

```rust
let network = config.spawn_native().await?;     // local processes
let network = config.spawn_k8s().await?;        // Kubernetes
let network = config.spawn_docker().await?;     // Docker daemon
```

`spawn_*` returns `Result<Network<LocalFileSystem>, OrchestratorError>`. The future resolves once the network is "ready" — relay producing blocks, paras registered.

## Network — runtime operations

```rust
let alice = network.get_node("alice")?;
let client = alice.wait_client::<subxt::PolkadotConfig>().await?;

// metrics
let value = alice.reports("block_height{status=\"best\"}").await?;
alice.assert("block_height{status=\"best\"}", 10).await?;   // panics-on-fail style

// process control
alice.pause().await?;
alice.resume().await?;
alice.restart(None).await?;   // None = same args; Some(...) = new args

// runtime additions
network.add_node("dave", AddNodeOptions {
    rpc_port: Some(9444),
    is_validator: true,
    ..Default::default()
}).await?;

network.add_collator("col02", AddCollatorOptions {
    command: Some("polkadot-parachain".try_into()?),
    ..Default::default()
}, /* para_id */ 100).await?;
```

`Network` is `Drop`-aware in tests — when it goes out of scope the underlying processes/pods are torn down. **Don't `mem::forget` it or hold it past the test boundary.**

## Attaching to an already-running network

```rust
use zombienet_sdk::AttachToLive;
use zombienet_sdk::AttachToLiveNetwork;

let net = AttachToLiveNetwork::attach_native("/tmp/zn-test/zombie.json").await?;
```

Useful for running multiple test suites against one long-lived network during local development. You need to supply the `zombie.json` file of the running network.

## subxt and subxt_signer

Re-exported as-is. Use `subxt::PolkadotConfig` for relay/para chains by default; use `subxt::SubstrateConfig` for legacy / non-Polkadot Substrate chains. For signing, `subxt_signer::sr25519::dev::alice()` and friends give you the standard well-known dev keys.

## What this reference doesn't cover

- Custom `Provider` implementations
- The `Orchestrator` lower-level type (you almost never need it directly)
- Test runner DSL (`*.zndsl`)
- The legacy v1 zombienet API

For those, read the source.
