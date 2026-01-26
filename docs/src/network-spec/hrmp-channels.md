# HRMP Channels

Cross-chain messaging between parachains.

### TOML

```toml
[[hrmp_channels]]
sender = 1000
recipient = 2000
max_capacity = 8
max_message_size = 512

[[hrmp_channels]]
sender = 2000
recipient = 1000
max_capacity = 8
max_message_size = 512
```

### Builder

```rust
let config = NetworkConfigBuilder::new()
    .with_relaychain(|r| { /* ... */ })
    .with_parachain(|p| p.with_id(1000).with_collator(|c| c.with_name("col1")))
    .with_parachain(|p| p.with_id(2000).with_collator(|c| c.with_name("col2")))
    .with_hrmp_channel(|h| h.with_sender(1000).with_recipient(2000))
    .with_hrmp_channel(|h| h.with_sender(2000).with_recipient(1000))
    .build()
    .unwrap();
```

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `sender` | Number | — | **Required.** Sending parachain ID |
| `recipient` | Number | — | **Required.** Receiving parachain ID |
| `max_capacity` | Number | 8 | Maximum messages in channel |
| `max_message_size` | Number | 512 | Maximum message size in bytes |
