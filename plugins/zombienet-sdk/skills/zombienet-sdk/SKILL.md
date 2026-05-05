---
name: zombienet-sdk
description: Use this skill whenever the user is working with zombienet or zombienet-sdk — writing or editing TOML network configs (relay chains, parachains, HRMP channels, node overrides, resource limits), authoring Rust integration tests that spawn ephemeral Polkadot/Substrate networks via NetworkConfigBuilder, or diagnosing failing zombienet runs (spawn timeouts, port-forward errors, parachains not producing blocks, session-0 issues, missing binaries/images). Trigger even when the user only says "zombienet" without specifying a sub-task, mentions a *.toml network config, references NetworkConfigBuilder / spawn_native / spawn_k8s / spawn_docker, talks about spinning up a local relay+parachain for testing, or pastes a zombienet error message or log snippet. Prefer this skill over generic Rust or TOML guidance whenever Polkadot test networks are involved.
---

# zombienet-sdk

zombienet-sdk spawns short-lived Polkadot/Substrate networks (relay chain + parachains) for integration testing. Networks are described declaratively (TOML or a Rust fluent builder) and run on one of three providers: `native` (local processes), `k8s` (Kubernetes pods), or `docker`.

This skill helps with three things — writing configs, writing Rust tests, and debugging failed runs. Pick the workflow below that matches what the user is doing. Don't run all three; pick one.

## When in doubt, read the source

The SDK changes faster than any external doc. **Real example files in `crates/examples/examples/` are the source of truth.** Before answering anything non-trivial, skim a relevant example:

- `crates/examples/examples/simple_network_example.rs` — load TOML + spawn + subscribe to blocks
- `crates/examples/examples/small_network_with_para.rs` — full builder, runtime node ops, metrics assertions
- `crates/examples/examples/configs/*.toml` — every config shape (groups, resources, wasm overrides, HRMP, etc.)

Public surface lives in `crates/sdk/src/lib.rs`. Errors live in `crates/orchestrator/src/errors.rs`. Builders live in `crates/configuration/src/`.

## Workflow A — Writing/editing a TOML network config

TOML is the user-facing way to describe a network. The runtime parses it into the same `NetworkConfig` the Rust builder produces.

**Minimal relay + parachain shape:**

```toml
[settings]
timeout = 1000

[relaychain]
chain = "rococo-local"
default_command = "polkadot"

[[relaychain.nodes]]
name = "alice"
validator = true

[[relaychain.nodes]]
name = "bob"
validator = true

[[parachains]]
id = 100
cumulus_based = true

[[parachains.collators]]
name = "collator01"
command = "polkadot-parachain"
```

**Key rules to apply:**
- Validators MUST live under `[relaychain]`; collators MUST live under `[[parachains]]`. Do not invent `[[validators]]` or `[[collators]]` at top level — they will be silently ignored or rejected.
- `chain` accepts `rococo-local`, `westend-local`, `paseo-local`, `kusama-local`, `polkadot-local`, or a path to a chain spec JSON.
- `cumulus_based = true` is correct for almost every modern parachain. Only set `false` for legacy adder/undying/etc. test collators.
- For a node group instead of named nodes, use `[[relaychain.node_groups]]` with `count = N` (see `crates/examples/examples/configs/simple-group-nodes.toml`).
- For Kubernetes-only fields like `[relaychain.nodes.resources]`, the config will fail to spawn under the `native` provider. Don't add resource limits unless the user is on k8s.
- HRMP channels go at the top level, not under a parachain — see references/toml-schema.md.

**For anything beyond the minimum** (HRMP, wasm/genesis overrides, resource limits, port pinning, env vars, command args, group nodes, registration strategy, async-backing config) → read `references/toml-schema.md`. Don't guess field names; they're easy to get wrong (e.g., it's `default_command` not `command` at the relay level; `cumulus_based` not `cumulus-based`).

After writing or editing a config, validate it by either pointing the user at `cargo run --example <some_loader>` or by writing a tiny Rust test that calls `NetworkConfig::load_from_toml("path/to/config.toml")?` — a parse error is far better feedback than a spawn failure.

## Workflow B — Writing a Rust integration test

The Rust API mirrors TOML one-to-one but is type-checked. Use it when the test needs to interact with the network after spawn (subscribe to blocks, assert on metrics, add nodes at runtime, send extrinsics via subxt).

**Canonical shape:**

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt, subxt};
use futures::StreamExt;

#[tokio::test(flavor = "multi_thread")]
async fn my_test() -> Result<(), anyhow::Error> {
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|n| n.with_name("alice"))
                .with_node(|n| n.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(true)
                .with_collator(|c| c.with_name("col01").with_command("polkadot-parachain"))
        })
        .build()
        .map_err(|errs| anyhow::anyhow!("{errs:?}"))?
        .spawn_native()
        .await?;

    let alice = network.get_node("alice")?;
    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);
    while let Some(b) = blocks.next().await {
        println!("finalized #{}", b?.header().number);
    }
    Ok(())
}
```

**Things that consistently trip people up:**
- `.build()` returns `Result<NetworkConfig, Vec<...>>` — the error is a *vector* of validation errors, not a single one. Map it before `?`.
- `spawn_native()` / `spawn_k8s()` / `spawn_docker()` come from the `NetworkConfigExt` trait — you must `use zombienet_sdk::NetworkConfigExt`.
- `network.get_node(name)?` returns the same node type whether you spawned native or k8s. The `wait_client()` future is the right way to wait for RPC readiness — don't `tokio::time::sleep`.
- Use `node.assert("metric_name", value)` and `node.reports("metric_name")` for Prometheus-based assertions instead of polling RPC.
- Add nodes/collators after spawn with `network.add_node(...)` / `network.add_collator(..., para_id)`.

For the full builder API surface (every method, every closure, every override), read `references/builder-api.md`.

## Workflow C — Debugging a failing run

The user will usually paste an error or log fragment. Match it against the patterns below before going deep. **Do not invent fixes — most zombienet failures are one of a small number of recurring causes.**

**Decision flow:**

1. **Read the actual error**, including the wrapped cause. `OrchestratorError` types in `crates/orchestrator/src/errors.rs` carry useful detail; the top-level message often hides it.
2. Check the failing node's log: under `native`, look for `<base_dir>/<node_name>.log` (the spawn output prints the base dir). Under k8s, `kubectl logs -n <ns> <pod>`. The relay/para process logs are usually more informative than zombienet's wrapper.
3. Match the symptom to the playbook in `references/debugging.md` — it covers spawn timeouts, port-forward conflicts, "parachain doesn't produce blocks" (often the session-0 / async-backing issue, fixed by `.with_override_session_0(true)`), missing binaries, image/version mismatches, and provider-specific quirks.
4. If nothing matches, ask the user: which provider? what version of `polkadot` and `polkadot-parachain` (try `--version`)? was this working previously, and if so what changed?

**Never recommend `--no-verify`, disabling tests, or "just bump the timeout" without understanding why the spawn is slow.** A 10-minute spawn usually means something is genuinely wrong (image pull, chain spec generation, validator coordination), not that the timeout is too tight.

## References

- `references/toml-schema.md` — every TOML field, with examples for HRMP, overrides, resources, async-backing, registration strategies, group nodes.
- `references/builder-api.md` — every `NetworkConfigBuilder` / `RelaychainConfigBuilder` / `ParachainConfigBuilder` / `NodeConfigBuilder` method.
- `references/debugging.md` — symptom → cause → fix table for the recurring failure modes.
