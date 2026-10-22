# JAM chain

A `[jamchain]` takes the place of the relaychain: a JAM network is rooted in its JAM nodes, and
parachains can be anchored to it.

### TOML

```toml
[jamchain]
id = "dev"
default_command = "polkajam"
protocol_params_type = "tiny"   # tiny (6 validators) | mini (78) | full (1023)

[[jamchain.nodes]]
name = "jam0"
mode = "validator"
# ... one entry per validator, plus any `ordinary` / `proxy` nodes
```

### Builder

```rust
use zombienet_sdk::NetworkConfigBuilder;

let config = NetworkConfigBuilder::new()
    .with_jamchain(|j| {
        let mut j = j.with_id("dev").with_validator(|n| n.with_name("jam0"));
        for i in 1..6 {
            j = j.with_validator(|n| n.with_name(&format!("jam{i}")));
        }
        j.with_ordinary(|n| n.with_name("jam-or"))
    })
    .build()
    .unwrap();
```

## Chain spec

By default zombienet builds the JIP-4 chain spec itself, with
[`jam-chainspec`](https://crates.io/crates/jam-chainspec). The genesis is the same one the node's
`gen-spec` writes: the network's validators, the **bootstrap service** as service `0` (hosting
the null authorizer) and the null authorizer on every core.

The bootstrap service comes from `[jamchain.genesis] bootstrap_service` (a path or a url), or is
embedded in zombienet with the **`jam` feature** (of `zombienet-sdk`, `zombienet-orchestrator` or
the CLI), so no extra file is needed:

```toml
zombienet-sdk = { version = "...", features = ["jam"] }
```

The feature builds it from the
[`jam-bootstrap-service`](https://crates.io/crates/jam-bootstrap-service) crate when zombienet is
compiled, which needs the `rust-src` component (`rustup component add rust-src`); it is off by
default so other users don't need it. To embed a prebuilt blob instead, set
`ZOMBIE_JAM_BOOTSTRAP_SERVICE_BLOB` to its path when building with the feature.

Without the feature and without `bootstrap_service`, building the spec fails and says so (unless
`with_defaults = false`).

The built-in builder needs exactly as many validator nodes as the protocol parameters call for
(6 for `tiny`, 78 for `mini`, 1023 for `full`).

How the spec is produced:

| Set | Spec |
|---|---|
| `chain_spec_path` | Used as given (a path or a url). |
| `chain_spec_command` or `genesis_overrides` | Generated with `<command> gen-spec <config> <spec>`, from the `jam_config.json` zombienet writes. |
| nothing (default) | Built in process, customized by `[jamchain.genesis]`. |

`genesis` can't be combined with the other three, nor `chain_spec_path` with the generator
fields.

### Customizing the genesis

```toml
[jamchain.genesis]
# Replace the embedded bootstrap service with another blob (a path or a url).
# bootstrap_service = "./blobs/jam-bootstrap-service.jam"
# Don't add the bootstrap service nor the null authorizer queues.
# with_defaults = false
# A native jam-chainspec JSON config, layered under the fields below. Its `id`,
# `genesis_validators` and `protocol_parameters` are replaced by zombienet's.
# config_file = "./my-chainspec-config.json"
# A full protocol parameters object, instead of the `protocol_params_type` preset.
# protocol_parameters = { ... }

[jamchain.genesis.services.5]
code = "./blobs/my-service.jam"          # a path or a url
balance = "min+1000000"                  # 1000 | "max" | "min" | "min+<amount>"
min_item_gas = 10
min_memo_gas = 10
deposit_offset = 0                       # an amount, or "max"
preimages = ["./blobs/my-authorizer.jam"]
storage = { "0x6772656574696e67" = "0x68656c6c6f" }

[jamchain.genesis.auth_queues]
0 = [{ code = "./blobs/my-authorizer.jam", config = "0x" }]   # hashed by zombienet
1 = ["null"]                                                   # or a 0x-prefixed hash

[jamchain.genesis.privileges]
manager = 0        # alias: bless
delegator = 0      # alias: designate
registrar = 0      # alias: register
assign = { 0 = 5 }
always_acc = { 5 = 1000000 }
```

Relative paths are resolved against the network definition's directory. Balances that don't fit
a TOML integer (like `u64::MAX`) are written as strings, or as `"max"`.

```rust
use zombienet_sdk::{jam::JamAuthorizer, NetworkConfigBuilder};

let config = NetworkConfigBuilder::new()
    .with_jamchain(|j| {
        j.with_id("dev")
            .with_genesis(|g| {
                g.with_service(5, |s| {
                    s.with_code("./blobs/my-service.jam")
                        .with_balance(zombienet_sdk::jam::JamBalance::MinimumPlus(1_000_000))
                        .with_preimage("./blobs/my-authorizer.jam")
                        .with_storage(b"greeting".to_vec(), b"hello".to_vec())
                })
                .with_auth_queue(0, [JamAuthorizer::code("./blobs/my-authorizer.jam")])
                .with_auth_queue(1, [JamAuthorizer::Null])
                .with_assigner([0], 5)
                // .with_bootstrap_service("https://example.com/jam-bootstrap-service.jam")
            })
            .with_validator(|n| n.with_name("jam0"))
            // ... 6 validators for tiny
    })
    .build()
    .unwrap();
```

The network's base directory gets `jam_spec.json` and, for a spec built in process,
`jam_genesis.json`: the genesis that went into it, with the code hash of every service and where
the bootstrap service came from.

The bootstrap service must suit the node: the embedded one is built from the
`jam-bootstrap-service` version matching the `jam-*` crates zombienet uses. For a node on another version, point
`bootstrap_service` at its blob, or generate the spec with `chain_spec_command = "polkajam"`.

The protocol parameters are process-wide, so all the JAM networks a process builds in process
must use the same set.

The SDK also exports the embedded blobs for tests: `zombienet_sdk::jam::NULL_AUTHORIZER_BLOB`,
`null_authorizer_hash()`, `embedded_bootstrap_service()` and, with the `jam` feature,
`BOOTSTRAP_SERVICE_BLOB`, `BOOTSTRAP_SERVICE_HASH` and `BOOTSTRAP_SERVICE_SOURCE`.
