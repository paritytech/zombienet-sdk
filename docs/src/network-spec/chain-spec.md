# Chain Spec Generation

Priority: `chain_spec_path` > `chain_spec_runtime` > `chain_spec_command` > auto-generation.

### Pre-existing

```toml
chain_spec_path = "/path/to/chain-spec.json"  # or URL
```

### Via Command

```toml
chain_spec_command = "polkadot build-spec --chain {chain} --disable-default-bootnode"
```

### Via Runtime

```toml
[relaychain.chain_spec_runtime]
src = "/path/to/runtime.wasm"
preset = "development"  # optional
```

Common presets: `development`, `local`
