# Your First Network

### Using TOML

Create `network.toml`:

```toml
[relaychain]
chain = "rococo-local"
default_command = "polkadot"

[[relaychain.nodes]]
name = "alice"

[[relaychain.nodes]]
name = "bob"
```

Spawn with the CLI:

```bash
zombie-cli spawn network.toml --provider native
# or: --provider docker
```

### Programmatically

The same network in Rust:

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    let alice = network.get_node("alice")?;
    println!("Alice WS: {}", alice.ws_uri());

    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

### Adding a Parachain

#### TOML

Extend `network.toml`:

```toml
[relaychain]
chain = "rococo-local"
default_command = "polkadot"

[[relaychain.nodes]]
name = "alice"

[[relaychain.nodes]]
name = "bob"

[[parachains]]
id = 1000
cumulus_based = true

    [[parachains.collators]]
    name = "collator01"
    command = "polkadot-parachain"
```

#### Programmatically

```rust
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(1000)
                .cumulus_based(true)
                .with_collator(|c| {
                    c.with_name("collator01")
                        .with_command("polkadot-parachain")
                })
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    let collator = network.get_node("collator01")?;
    println!("Collator WS: {}", collator.ws_uri());

    tokio::signal::ctrl_c().await?;
    Ok(())
}
```
