# zombienet TOML schema reference

Authoritative source: `crates/configuration/src/` and the example configs in `crates/examples/examples/configs/`. When in doubt, grep there. This file summarises the common fields; it is not exhaustive.

## Top-level structure

```toml
[settings]                 # global settings (optional)
[relaychain]               # required, exactly one
[[relaychain.nodes]]       # 1+ named nodes
[[relaychain.node_groups]] # 0+ node groups (alternative to nodes)
[[parachains]]             # 0+ parachains
[[parachains.collators]]   # 1+ named collators per parachain
[[hrmp_channels]]          # 0+ HRMP channels (top-level, not nested)
```

## `[settings]`

| Field | Type | Notes |
|---|---|---|
| `timeout` | int (seconds) | Global spawn timeout. Default ~1000. Increase only after diagnosing slow spawns. |
| `node_spawn_timeout` | int | Per-node spawn timeout. |
| `bootnode` | bool | Add an explicit bootnode. |
| `local_ip` | string | Override the local IP advertised to peers. |

## `[relaychain]`

| Field | Type | Notes |
|---|---|---|
| `chain` | string | `rococo-local`, `westend-local`, `paseo-local`, `kusama-local`, `polkadot-local`, or a path to a chain-spec JSON. |
| `default_command` | string | `polkadot` typically. Used when a node doesn't override. |
| `default_image` | string | Container image (k8s/docker only). |
| `default_args` | array<string> | CLI args appended to every node. |
| `chain_spec_path` | string | Use a pre-generated chain spec instead of regenerating. |
| `chain_spec_command` | string | Command to generate the chain spec (advanced). |
| `random_nominators_count` | int | Auto-create N nominator accounts. |
| `max_nominations` | int | Per-account nomination cap. |
| `runtime_genesis_patch` | inline table or path | JSON patch applied to the genesis runtime config. |
| `default_resources` | table | k8s resource defaults (see Resources below). |

### Recent additions (check `crates/configuration/src/relaychain.rs` for the current set):

- `override_session_0 = true` — overrides `ParaSessionInfo.sessions(0)`, `ParaScheduler.ValidatorGroups`, and core descriptors so parachains can produce blocks immediately. Pair with `paras_production_at = 0`-style settings when the relay needs to assign cores in raw genesis. See commit `5b46fff` and `7c45ef5` for the canonical examples.

## `[[relaychain.nodes]]`

| Field | Type | Notes |
|---|---|---|
| `name` | string (required) | Unique within the network. Used by `network.get_node(name)`. |
| `command` | string | Overrides `default_command`. |
| `image` | string | Overrides `default_image`. |
| `args` | array<string> | Per-node CLI args. |
| `validator` | bool | Default `true` for relay nodes. |
| `invulnerable` | bool | Adds to invulnerable validator set. |
| `balance` | int | Initial balance. |
| `bootnodes` | array<string> | Custom bootnode multiaddrs. |
| `rpc_port` | int | Pin the RPC port (otherwise random). Useful for reproducibility, dangerous for parallel runs. |
| `prometheus_port` | int | Pin the metrics port. |
| `p2p_port` | int | Pin the libp2p port. |
| `db_snapshot` | string | Path or URL to a DB snapshot to seed from. |
| `env` | array of `{name, value}` | Environment variables. |
| `resources` | table | k8s resources (see below). Causes spawn failure under `native`. |
| `add_to_bootnodes` | bool | Whether peers connect to this node by default. |

## `[[relaychain.node_groups]]`

Alternative to `[[relaychain.nodes]]` when you want N identical nodes:

```toml
[[relaychain.node_groups]]
name = "validators"
count = 4
validator = true
command = "polkadot"
```

The orchestrator generates names like `validators-0`, `validators-1`, ... See `crates/examples/examples/configs/simple-group-nodes.toml`.

## `[[parachains]]`

| Field | Type | Notes |
|---|---|---|
| `id` | int (required) | Para ID. Must be unique. |
| `chain` | string | Optional chain spec name. |
| `cumulus_based` | bool | `true` for modern parachains; `false` only for legacy test collators (adder, undying). |
| `default_command` | string | `polkadot-parachain` typically. |
| `default_image` | string | Container image. |
| `default_args` | array<string> | CLI args appended to every collator. |
| `add_to_genesis` | bool | Register the para in genesis (vs. on-the-fly registration). |
| `register_para` | bool | Register via extrinsic at runtime. |
| `onboard_as_parachain` | bool | Onboard immediately rather than as a parathread. |
| `genesis_wasm_path` | string | Pre-built genesis wasm. |
| `genesis_wasm_generator` | string | Command to generate the wasm. |
| `genesis_state_path` | string | Pre-built genesis state. |
| `genesis_state_generator` | string | Command to generate the state. |
| `wasm_override` | string | Replace the runtime wasm post-spawn (live upgrade testing). |
| `runtime_genesis_patch` | inline table or path | JSON patch over genesis. |
| `chain_spec_path` | string | Pre-generated chain spec. |
| `chain_spec_command` | string | Command to generate the chain spec. |

## `[[parachains.collators]]`

Same shape as `[[relaychain.nodes]]` (name, command, image, args, env, ports, resources, ...) but `validator` defaults to `false` and the role is "collator".

## `[[hrmp_channels]]`

```toml
[[hrmp_channels]]
sender = 100
recipient = 101
max_capacity = 8
max_message_size = 512
```

Top-level, not nested under a parachain. Both `sender` and `recipient` must already be declared as parachains in the same config.

## Resources (k8s only)

```toml
[relaychain.default_resources.requests]
memory = "512Mi"
cpu = "250m"

[relaychain.default_resources.limits]
memory = "1Gi"
cpu = "500m"
```

Or per-node: `[relaychain.nodes.resources.requests]` etc.

**Do not put resources in a config that will be run with the `native` provider** — it will fail validation with `InvalidConfigForProvider`.

## Worked examples

The example configs are the best reference because they actually parse and run:

- `crates/examples/examples/configs/simple.toml` — minimal relay + adder collator
- `crates/examples/examples/configs/simple-group-nodes.toml` — node groups
- `crates/examples/examples/configs/resource_limits.toml` — k8s resources
- `crates/examples/examples/configs/wasm-override.toml` — runtime upgrade testing
- `crates/examples/examples/configs/small-network.toml` — multi-validator + parachain

## Common mistakes to flag in review

- Putting `[[validators]]` or `[[collators]]` at the top level (they belong under `[relaychain]` / `[[parachains]]`).
- Mixing snake_case and kebab-case field names. Always snake_case.
- Setting `cumulus_based = false` for a modern Substrate parachain (causes weird block-production failures).
- Specifying `image` without `command`, or vice versa, when the defaults don't apply.
- Pinning `rpc_port` / `p2p_port` for parallel test runs (port collisions).
- Adding HRMP channels between paras that aren't both declared.
- Resource limits with the native provider.
