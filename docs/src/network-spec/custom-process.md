# Custom Processes

Allow to set custom processes to spawn after the network is up.

### TOML

```toml
[[custom_processes]]
name = "eth-rpc"
command = "eth-rpc"
args = [ "--flag", "--other=1" ]
env = [
    { name = "RUST_LOG", value = "info" }
]

[[custom_processes]]
name = "other"
command = "some-binary"
args = [ "--flag", "--other-flag" ]
```

### Builder

```rust
let config = NetworkConfigBuilder::new()
    .with_relaychain(|r| { /* ... */ })
    .with_custom_process(|c| c.with_name("eth-rpc").with_command("eth-rpc"))
```

```rust
let cp = CustomProcessBuilder::new()
    .with_name("eth-rpc")
    .with_command("eth-rpc")
    .with_args(vec!["--flag".into()])
    .with_env(vec![("Key", "Value")])
    .build()
    .unwrap();
```

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `name` | String | — | **Required.** Name of the process |
| `command` | String | — | **Required.** Command to execute |
| `image` | String | - | Container image |
| `args` | Array | — | CLI arguments |
| `env` | Array | — | Environment variables as `{name, value}` pairs |
