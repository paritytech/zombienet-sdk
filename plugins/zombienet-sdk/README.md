# zombienet-sdk Claude Code plugin

A [Claude Code](https://docs.claude.com/en/docs/claude-code) plugin that ships a single **skill** to help with:

- Writing and editing zombienet TOML network configs (relay chains, parachains, HRMP channels, overrides, resource limits)
- Authoring Rust integration tests using `NetworkConfigBuilder` and the `spawn_native` / `spawn_k8s` / `spawn_docker` APIs
- Debugging failed zombienet runs (spawn timeouts, port conflicts, parachains not producing blocks, session-0 issues, missing binaries)

The skill is auto-triggered whenever Claude detects a zombienet-related task — you don't need to invoke it explicitly.

## Install

The plugin is hosted in the same repo as zombienet-sdk itself, which is its own marketplace.

In Claude Code:

```
/plugin marketplace add paritytech/zombienet-sdk
/plugin install zombienet-sdk@zombienet-sdk
```

That's it. After install, ask Claude anything zombienet-related — for example:

- "Write a zombienet TOML for a rococo-local relay with two parachains and an HRMP channel between them."
- "My parachain isn't producing blocks after spawn — here's the log…"
- "Show me how to write a Rust test that spawns a network, waits for 5 finalized blocks on alice, then asserts a metric on collator01."

## Update

```
/plugin marketplace update zombienet-sdk
/plugin install zombienet-sdk@zombienet-sdk   # re-installs the latest version
```

## Uninstall

```
/plugin uninstall zombienet-sdk@zombienet-sdk
/plugin marketplace remove zombienet-sdk
```

## Local development

If you're iterating on the skill itself, point Claude Code at your local checkout instead of the published repo:

```
/plugin marketplace add /absolute/path/to/zombienet-sdk
/plugin install zombienet-sdk@zombienet-sdk
```

After editing the skill, run `/plugin marketplace update zombienet-sdk` and reinstall to pick up changes. (You may need `/reload-plugins` in some Claude Code versions.)

## Layout

```
zombienet-sdk/                          (this repo)
├── .claude-plugin/
│   └── marketplace.json                (declares this repo as a marketplace)
└── plugins/
    └── zombienet-sdk/
        ├── .claude-plugin/
        │   └── plugin.json             (plugin manifest)
        ├── README.md                   (this file)
        └── skills/
            └── zombienet-sdk/
                ├── SKILL.md            (entry point, always loaded when triggered)
                └── references/         (loaded on demand)
                    ├── toml-schema.md
                    ├── builder-api.md
                    └── debugging.md
```

The skill follows progressive disclosure: `SKILL.md` is concise and points at `references/` files for deeper material, so context only fills up when needed.

## Contributing

Found a recurring zombienet failure mode the skill doesn't handle? Open a PR against `references/debugging.md`. New SDK API surface? Update `references/builder-api.md` and `SKILL.md`'s "Workflow B" example. Keep `SKILL.md` itself short — the goal is a fast-loading entry point that delegates to references.
