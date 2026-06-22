# MCP Diagnostics

`zombie-mcp` lets Codex or Claude Code debug a zombienet run that you already started. It is read-only: it uses `zombie.json` as the run handle and inspects logs, liveness, metrics, and block production, but it does not start or stop the network.

The same diagnostics also run **without an LLM client** as a plain CLI (see [CLI mode](#cli-mode)). The two layers share one core:

- **CLI / core (deterministic).** `zombie-mcp diagnose` runs the baked detection — config validation, log-pattern matching, liveness, metrics, block production — and prints a JSON report. It is reproducible and CI-friendly; no model is involved.
- **LLM via MCP (additive).** The MCP frontend only adds what a deterministic tool cannot: natural-language entry, correlating evidence with repository source, and reasoning about errors that fall outside the baked log patterns. It orchestrates the same core functions; it does not change their logic.

## CLI mode

For humans and CI, run the diagnostics directly — no client to install, no model required:

```sh
# Auto-discover the most recent run and diagnose it.
cargo run -p zombie-mcp -- diagnose --auto

# Diagnose a specific run by its zombie.json path.
cargo run -p zombie-mcp -- diagnose --zombie-json /path/to/zombie.json

# Gate CI: exit 1 when the report status is `failed`.
cargo run -p zombie-mcp -- diagnose --auto --fail-on-error
```

The JSON `DiagnosticReport` is written to stdout; tracing logs stay on stderr, so you can pipe the report straight into `jq` (which also pretty-prints it) or a CI artifact. `--auto` picks the newest run found by the same discovery the LLM used (`find_recent_runs`), then diagnoses it.

The MCP server is an opt-in `mcp` feature (on by default). To build just the core + CLI without the MCP server stack:

```sh
cargo build -p zombie-mcp --no-default-features
```

## Install

Run one of these once from the repository root, depending on your client:

```sh
# Codex
cargo run -p zombie-mcp -- install codex --force

# Claude Code
cargo run -p zombie-mcp -- install claude --force
```

Restart the client after installing so the MCP server is loaded.

## Debug A Run

Start your network however you normally do. If it hangs or fails, paste this into Codex:

```text
Debug my zombienet run with zombie-mcp.
```
