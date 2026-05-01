# Debugging zombienet-sdk runs

A symptom-first playbook. Most zombienet failures are one of a handful of recurring causes — match the symptom first, then dig.

## Step 0 — Read the actual error

The top-level error printed in the test runner's stderr is usually a wrapped `OrchestratorError`. The wrapping discards detail. Find the inner cause:

- `OrchestratorError::InvalidConfig(msg)` — the TOML / builder produced an invalid config. Fix the config; don't retry.
- `OrchestratorError::InvalidConfigForProvider { provider, reason }` — config has fields the provider doesn't support (most often k8s `resources` under `native`).
- `OrchestratorError::InvalidNodeConfig { node, field }` — one specific node's setting is wrong.
- `OrchestratorError::GlobalTimeOut(secs)` — spawn took longer than the configured timeout. **Don't just bump the timeout.** See the timeout section below.
- `OrchestratorError::GeneratorError(_)` — chain spec / genesis state / wasm generation failed. Check the generator command output in the base dir.
- `OrchestratorError::ProviderError(_)` — provider-level failure (port, image pull, kubeconfig, docker daemon).
- `OrchestratorError::FileSystemError(_)` — write or read on the base dir failed (disk full, permissions).

The full taxonomy is in `crates/orchestrator/src/errors.rs` — read it if the error doesn't match the above.

## Step 1 — Find the per-node logs

Zombienet's wrapper output is rarely the most informative. The relay/para process logs almost always are.

- **Native provider** — logs go under `<base_dir>/<node_name>.log`. The base dir is printed when spawn starts (look for "Base dir:" or similar). It defaults to a `tmp` location unless overridden via `.with_base_dir(...)` in `GlobalSettings`.
- **k8s provider** — `kubectl logs -n <namespace> <pod-name>`. The namespace is printed at spawn.
- **docker provider** — `docker logs <container-name>`.

Look at the **last 100 lines** of the failing node's log first; the error is almost always near the bottom.

## Symptom playbook

### "Spawn timed out after N seconds" / `GlobalTimeOut`

- **Image pull is slow (k8s/docker)** — check `kubectl describe pod` or `docker pull` manually. Fix: pre-pull, use a closer registry, or pin a specific tag.
- **Chain spec generation is slow** — happens when the binary is debug-built or when generating from a heavy runtime. Fix: pre-generate with `chain_spec_path`/`chain_spec_command`.
- **Validators not finalizing** — relay produces blocks but doesn't finalize. Often misconfiguration (wrong `chain` value, mismatched binaries between validators). Check the validator logs for grandpa errors.
- **Para waiting for session** — see "Parachain doesn't produce blocks" below. This is by far the most common cause of "spawn never finishes" on recent runtime versions.

Only after ruling out the above is bumping the timeout the right answer.

### "Parachain doesn't produce blocks" / spawn hangs after relay starts

This is the canonical recent regression. On modern runtimes with async backing and core scheduling, parachains can't produce blocks in session 0 because `ParaSessionInfo.sessions(0)` and `ParaScheduler.ValidatorGroups` aren't populated until session 1.

**Fix:** in the relay config, set `override_session_0 = true` (TOML) or `.with_override_session_0(true)` (builder). See commits `5b46fff` and `7c45ef5` for the canonical changes that introduced this. Also check `crates/configuration/src/relaychain.rs` for any related options around `paras_production_at` / `assign_cores_in_raw_genesis`.

If the user is on an older SDK version (pre-0.4.10) and hits this, they need to bump the SDK version, not edit their config.

### "Error creating port-forward" / port already in use

- k8s/docker port-forward failed because the host port is occupied. Common causes: previous test run didn't tear down (orphan process), parallel tests pinning the same port, system service on the port.
- Fix: don't pin `rpc_port` / `p2p_port` / `prometheus_port` for parallel runs. Let the orchestrator allocate. If you must pin (for reproducibility), `lsof -i :<port>` to find the conflict.

### "command not found" / "No such file or directory" (native provider)

- The native provider expects every `command` (`polkadot`, `polkadot-parachain`, custom collators) to be on `$PATH` or specified as an absolute path. There is no automatic build/install.
- Fix: install the binary, or set `command = "/abs/path/to/binary"`, or build with `cargo build --release -p polkadot` and add `target/release` to `$PATH`.
- A subtler variant: the binary IS on `$PATH` but is the wrong version (e.g., a stable polkadot when the test needs a master build, or vice versa). `polkadot --version` to verify.

### "ImagePullBackOff" / "manifest unknown" (k8s/docker)

- The `default_image` or per-node `image` doesn't exist or isn't accessible.
- Fix: verify the image tag exists in the registry. For internal Parity images, check you have credentials. For public test images, common ones are `parity/polkadot:latest` and `parity/polkadot-parachain:latest` — but `:latest` rotates, so prefer pinned tags.

### `InvalidConfigForProvider`

- The config has fields one provider supports and another doesn't. Usually: k8s `resources` block on `native`, or `image` without `command` on `native` (no image to pull from).
- Fix: either remove the offending fields, or switch providers, or guard the fields behind a config variant.

### Validators connect but don't reach finality

- Most often a chain mismatch — different validators built from different commits, or different `chain` values (one validator on `rococo-local`, another somehow on `westend-local`). Check that all validators use the same `default_command` and `chain`.
- Less commonly, the genesis_runtime_patch invalidated something that finality depends on. Try without the patch and see if finality resumes.

### `subxt` errors (`Decode`, `Metadata`)

- The `polkadot` binary version doesn't match what subxt was built against. The relay chain runtime metadata changed.
- Fix: regenerate subxt-generated types (`subxt metadata` + `subxt codegen`), or use `subxt::dynamic` queries that don't require static codegen.

### "Network is dropped" / processes vanish mid-test

- The `Network` value went out of scope. `Drop` tears down the network.
- Fix: hold the `Network` for the lifetime of the test. Don't `let _ = ...` it or pass it to a function that consumes it before assertions run.

## Step 2 — Asking the user the right questions

When the symptom doesn't match anything above, before guessing, ask:

1. **Provider?** native / k8s / docker.
2. **Versions?** `polkadot --version`, `polkadot-parachain --version`, the zombienet-sdk version in `Cargo.toml`.
3. **Was this working before?** If yes, what changed — SDK bump, binary rebuild, runtime change, config edit?
4. **Full error and the failing node's log tail.** Not the wrapper error.

These four questions resolve most non-obvious failures.

## Step 3 — Don't reach for these

- **`--no-verify`** or skipping the test. The whole point is the test.
- **Bumping `timeout` blindly** without understanding why spawn is slow.
- **Disabling validators** to "simplify". Removes signal, doesn't fix anything.
- **`tokio::time::sleep` to wait for the network.** Use `wait_client()` or metric assertions.
