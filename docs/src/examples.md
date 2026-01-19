# Examples

Examples are in `crates/examples/examples/`. Run with:

```bash
cargo run --example <example_name>
```

---

## Basic Configuration

Start here to understand network setup patterns.

### Minimal Network

The simplest network configuration: a single relay chain node.

```rust
use zombienet_sdk::NetworkConfigBuilder;

fn main() {
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_validator(|node| node.with_name("alice").with_command("polkadot"))
        })
        .build();

    println!("{:?}", config.unwrap());
}
```

**Source:** [small_network_config.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/small_network_config.rs)

---

### Loading from TOML

Use TOML files for configuration that doesn't need programmatic control.

```rust
use zombienet_sdk::{NetworkConfig, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let network = NetworkConfig::load_from_toml("./config.toml")?
        .spawn_native()
        .await?;
    Ok(())
}
```

**Example TOML:**

```toml
[settings]
timeout = 1000

[relaychain]
default_image = "docker.io/parity/polkadot:latest"
chain = "rococo-local"
command = "polkadot"

  [[relaychain.nodes]]
  name = "alice"
  args = [ "--alice", "-lruntime=debug,parachain=trace" ]

  [[relaychain.nodes]]
  name = "bob"
  args = [ "--bob", "-lruntime=debug,parachain=trace" ]
```

**Source:** [simple_network_example.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/simple_network_example.rs)

---

## Parachains

Configure parachains for cross-chain testing. Parachains need at least one collator and can be registered in genesis or via extrinsic.

### Basic Setup

A cumulus-based parachain with automatic genesis registration.

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;
    Ok(())
}
```

**Source:** [small_network_with_para.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/small_network_with_para.rs)

---

### Adding Parachain to Running Network

Add a parachain dynamically after spawn. Use `register_parachain` instead for parachains spawned with `RegistrationStrategy::Manual`.

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let mut network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    let para_config = network
        .para_config_builder()
        .with_id(100)
        .with_default_command("polkadot-parachain")
        .with_collator(|c| c.with_name("col-100-1"))
        .build()?;

    network
        .add_parachain(&para_config, None, Some("new_para_100".to_string()))
        .await?;
    Ok(())
}
```

**Source:** [add_para.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/add_para.rs)

---

### Manual Registration

For parachains spawned with `RegistrationStrategy::Manual`:

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt, RegistrationStrategy};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let mut network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_registration_strategy(RegistrationStrategy::Manual)
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("test-parachain"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    network.register_parachain(2000).await?;
    Ok(())
}
```

**Source:** [register_para.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/register_para.rs)

---

### Multiple Parachains with Same ID

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt, RegistrationStrategy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_registration_strategy(RegistrationStrategy::Manual)
                .with_collator(|n| n.with_name("collator1").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    Ok(())
}
```

**Source:** [two_paras_same_id.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/two_paras_same_id.rs)

---

## Node Groups

Scale networks by creating multiple nodes with the same configuration. Node groups generate nodes with incremental names (e.g., `validator-1`, `validator-2`).

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node_group(|g| g.with_count(3).with_base_node(|b| b.with_name("validator")))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(true)
                .with_default_command("polkadot-parachain")
                .with_collator_group(|g| g.with_count(2).with_base_node(|b| b.with_name("collator")))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;
    Ok(())
}
```

**TOML:**

```toml
[relaychain]
chain = "rococo-local"
default_command = "polkadot"

[[relaychain.node_groups]]
name = "validator"
count = 3

[[parachains]]
id = 100

[[parachains.collator_groups]]
name = "collator"
command = "polkadot-parachain"
count = 2
```

**Source:** [big_network_with_group_nodes.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/big_network_with_group_nodes.rs)

---

## Providers

```rust
let network = config.spawn_native().await?;  // Local processes
let network = config.spawn_docker().await?;  // Docker containers
let network = config.spawn_k8s().await?;     // Kubernetes pods
```

### Custom Base Directory

```rust
use std::path::Path;
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

let config = NetworkConfigBuilder::new()
    .with_relaychain(|r| {
        r.with_chain("rococo-local")
            .with_default_command("polkadot")
            .with_validator(|node| node.with_name("alice"))
    })
    .with_global_settings(|g| g.with_base_dir(Path::new("/tmp/zombie-1")))
    .build()
    .unwrap();
```

**Source:** [small_network_with_base_dir.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/small_network_with_base_dir.rs)

---

## Advanced Features

Advanced configuration patterns for specific use cases.

### Database Snapshots

Start nodes with pre-existing chain data. Useful for testing migrations or resuming from a known state.

```rust
.with_relaychain(|r| {
    r.with_default_db_snapshot("https://storage.googleapis.com/zombienet-db-snaps/...")
        .with_validator(|node| node.with_name("alice"))
})
```

**Source:** [db_snapshot.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/db_snapshot.rs)

---

### Runtime Upgrades

```rust
use zombienet_sdk::tx_helper::{ChainUpgrade, RuntimeUpgradeOptions};

network
    .parachain(100)?
    .runtime_upgrade(RuntimeUpgradeOptions::new("/path/to/new_runtime.wasm".into()))
    .await?;
```

**Source:** [para_upgrade.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/para_upgrade.rs)

---

### Custom Chain Spec Runtime

```rust
.with_relaychain(|r| {
    r.with_chain("kusama-local")
        .with_chain_spec_runtime(
            "https://github.com/polkadot-fellows/runtimes/releases/download/v2.0.2/kusama_runtime.wasm",
            Some("local_testnet")
        )
        .with_validator(|node| node.with_name("alice"))
})
```

**Source:** [chain_spec_runtime_kusama.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/chain_spec_runtime_kusama.rs)

---

### Custom Genesis State Generator

For non-cumulus parachains:

```rust
.with_parachain(|p| {
    p.with_id(100)
        .cumulus_based(false)
        .with_genesis_state_generator("undying-collator export-genesis-state --pov-size=10000")
        .with_collator(|n| n.with_name("collator").with_command("undying-collator"))
})
```

**Source:** [genesis_state_generator_example.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/genesis_state_generator_example.rs)

---

### EVM-Based Parachains

```rust
.with_parachain(|para| {
    para.with_id(2000)
        .cumulus_based(true)
        .evm_based(true)
        .with_collator(|c| {
            c.with_name("evm-collator")
                .with_command("polkadot-parachain")
                .with_override_eth_key("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        })
})
```

**Source:** [evm_parachain_session_key.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/evm_parachain_session_key.rs)

---

### Keystore Key Types

```rust
.with_validator(|node| {
    node.with_name("alice")
        .with_keystore_key_types(vec!["aura", "gran", "babe"])
})
```

**Source:** [keystore_key_types.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/keystore_key_types.rs)

---

### Argument Removal

Use `-:` prefix to remove default arguments:

```toml
args = [ "-:--insecure-validator-i-know-what-i-do" ]
```

**Source:** [arg_removal.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/arg_removal.rs)

---

### Running Scripts on Nodes

```rust
use zombienet_sdk::RunScriptOptions;

let alice = network.get_node("alice")?;
let options = RunScriptOptions::new(&PathBuf::from("./test_script.sh"))
    .args(vec!["arg1".to_string()])
    .env(vec![("NODE_NAME".to_string(), "alice".to_string())]);

alice.run_script(options).await?;
```

**Source:** [test_run_script.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/test_run_script.rs)

---

## Interacting with Networks

Interact with running networks for testing and automation.

### Attaching to Live Network

Reconnect to a previously spawned network using the `zombie.json` state file.

```rust
use zombienet_sdk::{AttachToLive, AttachToLiveNetwork};

let network = AttachToLiveNetwork::attach_native("/tmp/zombie-1/zombie.json".into()).await?;
let alice = network.get_node("alice")?;
```

**Source:** [from_live.rs](https://github.com/paritytech/zombienet-sdk/blob/main/crates/examples/examples/from_live.rs)

---

### Using Subxt

```rust
use zombienet_sdk::subxt;

let client = network.get_node("alice")?.wait_client::<subxt::PolkadotConfig>().await?;
let mut blocks = client.blocks().subscribe_finalized().await?;
```

---

### Querying Metrics

```rust
let node = network.get_node("alice")?;

// Check condition
let is_10: bool = node.assert("block_height{status=\"best\"}", 10).await?;

// Get value
let role: f64 = node.reports("node_roles").await?;

// Wait for condition
node.wait_metric("block_height{status=\"best\"}", |b| b > 5_f64).await?;

// Wait with timeout
node.wait_metric_with_timeout("block_height{status=\"best\"}", |v| v >= 5_f64, 90_u32).await?;
```

---

### Adding Nodes Dynamically

```rust
use zombienet_sdk::{AddCollatorOptions, AddNodeOptions};

// Add validator
let opts = AddNodeOptions { rpc_port: Some(9444), is_validator: true, ..Default::default() };
network.add_node("new_validator", opts).await?;

// Add collator
let col_opts = AddCollatorOptions { command: Some("polkadot-parachain".try_into()?), ..Default::default() };
network.add_collator("new_collator", col_opts, 100).await?;
```
