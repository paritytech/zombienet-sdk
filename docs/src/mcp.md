# MCP Diagnostics

`zombie-mcp` lets Codex or Claude Code debug a zombienet run that you already started. It is read-only: it uses `zombie.json` as the run handle and inspects logs, liveness, metrics, and block production, but it does not start or stop the network.

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
