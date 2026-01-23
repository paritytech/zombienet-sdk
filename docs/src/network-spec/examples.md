# Complete Example

Relay chain with two parachains and HRMP channels.

### TOML

```toml
[settings]
timeout = 600

[relaychain]
chain = "rococo-local"
default_command = "polkadot"
default_args = ["-lruntime=debug"]

[[relaychain.nodes]]
name = "alice"

[[relaychain.nodes]]
name = "bob"

[[parachains]]
id = 1000

[[parachains.collators]]
name = "collator-1000"
command = "polkadot-parachain"

[[parachains]]
id = 2000

[[parachains.collators]]
name = "collator-2000"
command = "polkadot-parachain"

[[hrmp_channels]]
sender = 1000
recipient = 2000

[[hrmp_channels]]
sender = 2000
recipient = 1000
```

### Builder

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
                .with_collator(|c| c.with_name("collator-1000").with_command("polkadot-parachain"))
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_collator(|c| c.with_name("collator-2000").with_command("polkadot-parachain"))
        })
        .with_hrmp_channel(|h| h.with_sender(1000).with_recipient(2000))
        .with_hrmp_channel(|h| h.with_sender(2000).with_recipient(1000))
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    Ok(())
}
```
