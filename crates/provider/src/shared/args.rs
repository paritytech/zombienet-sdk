/// Flags injected by zombienet that must be preserved across `restart_with` calls.
const INTERNAL_FLAGS: &[&str] = &[
    "--no-telemetry",
    "--prometheus-external",
    "--validator",
    "--collator",
    "--unsafe-rpc-external",
    "--insecure-validator-i-know-what-i-do",
];

/// Key-value options injected by zombienet that must be preserved across `restart_with` calls.
const INTERNAL_OPTIONS: &[&str] = &[
    "--chain",
    "--name",
    "--node-key",
    "--base-path",
    "--rpc-port",
    "--prometheus-port",
    "--listen-addr",
    "--rpc-cors",
    "--rpc-methods",
    "--bootnodes",
    "--parachain-id",
];

/// Merge new user args into the existing args, preserving all zombienet-internal args.
///
/// The existing args contain both internal args (added by zombienet during initial spawn, e.g.
/// `--base-path`, `--chain`, `--node-key`, ports, etc.) and user-specified args. When
/// `restart_with` is called with new args, we must keep the internal ones untouched while
/// replacing only the user-specified portion.
///
/// For cumulus nodes the args contain a `--` separator followed by relay-chain args — that entire
/// section is always internal and is preserved as-is.
pub fn merge_args(existing: &[String], new_user_args: &[String]) -> Vec<String> {
    // Split off the cumulus relay-chain section (from `--` onward) — it is always internal.
    let separator_pos = existing.iter().position(|a| a == "--");
    let (before_sep, relay_section) = match separator_pos {
        Some(pos) => (&existing[..pos], &existing[pos..]),
        None => (existing, &[][..]),
    };

    // Walk `before_sep` and keep only internal args.
    let mut kept: Vec<String> = Vec::new();
    let mut i = 0;
    while i < before_sep.len() {
        let arg = &before_sep[i];
        if INTERNAL_FLAGS.contains(&arg.as_str()) {
            kept.push(arg.clone());
            i += 1;
        } else if INTERNAL_OPTIONS.contains(&arg.as_str()) {
            // Keep the key and its value (next element).
            kept.push(arg.clone());
            if i + 1 < before_sep.len() {
                kept.push(before_sep[i + 1].clone());
                i += 2;
            } else {
                i += 1;
            }
        } else {
            // User arg — skip it.
            i += 1;
        }
    }

    // Append new user args, then the relay-chain section.
    kept.extend_from_slice(new_user_args);
    kept.extend_from_slice(relay_section);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn preserves_internal_args_and_replaces_user_args() {
        let existing = vec![
            s("--chain"), s("/cfg/rococo-local.json"),
            s("--name"), s("validator-0"),
            s("--rpc-cors"), s("all"),
            s("--rpc-methods"), s("unsafe"),
            s("--node-key"), s("abc123"),
            s("--no-telemetry"),
            s("--prometheus-external"),
            s("--validator"),
            s("--prometheus-port"), s("9615"),
            s("--rpc-port"), s("9944"),
            s("--listen-addr"), s("/ip4/0.0.0.0/tcp/30333/ws"),
            s("--base-path"), s("/tmp/zombie/validator-0/data"),
            s("-lparachain=debug"),                 // user arg
            s("--experimental-collator-protocol"),  // user arg
        ];

        let new_user_args = vec![s("-lparachain=trace")];
        let result = merge_args(&existing, &new_user_args);

        // Internal args must be present.
        assert!(result.contains(&s("--base-path")));
        assert!(result.contains(&s("/tmp/zombie/validator-0/data")));
        assert!(result.contains(&s("--chain")));
        assert!(result.contains(&s("--node-key")));
        assert!(result.contains(&s("--no-telemetry")));
        assert!(result.contains(&s("--validator")));
        assert!(result.contains(&s("--rpc-port")));
        assert!(result.contains(&s("9944")));

        // New user arg must be present.
        assert!(result.contains(&s("-lparachain=trace")));

        // Old user args must be gone.
        assert!(!result.contains(&s("-lparachain=debug")));
        assert!(!result.contains(&s("--experimental-collator-protocol")));
    }

    #[test]
    fn preserves_cumulus_relay_section() {
        let existing = vec![
            s("--chain"), s("/cfg/1000.json"),
            s("--name"), s("collator-1000"),
            s("--rpc-cors"), s("all"),
            s("--rpc-methods"), s("unsafe"),
            s("--node-key"), s("abc123"),
            s("--no-telemetry"),
            s("--collator"),
            s("--base-path"), s("/tmp/zombie/collator/data"),
            s("-lparachain=debug"),  // user arg
            s("--"),                 // separator
            s("--base-path"), s("/tmp/zombie/collator/relay-data"),
            s("--chain"), s("/cfg/rococo-local.json"),
            s("--execution"), s("wasm"),
            s("--port"), s("30334"),
            s("--prometheus-port"), s("9616"),
        ];

        let new_user_args = vec![s("-lparachain=trace")];
        let result = merge_args(&existing, &new_user_args);

        // Collator's own internal args preserved.
        assert!(result.contains(&s("--base-path")));
        assert!(result.contains(&s("/tmp/zombie/collator/data")));
        assert!(result.contains(&s("--collator")));

        // New user arg present.
        assert!(result.contains(&s("-lparachain=trace")));

        // Old user arg gone.
        assert!(!result.contains(&s("-lparachain=debug")));

        // Relay-chain section fully preserved.
        assert!(result.contains(&s("--")));
        assert!(result.contains(&s("/tmp/zombie/collator/relay-data")));
        assert!(result.contains(&s("--execution")));
        assert!(result.contains(&s("wasm")));

        // Relay section must come after user args.
        let sep_pos = result.iter().position(|a| a == "--").unwrap();
        let user_pos = result.iter().position(|a| a == "-lparachain=trace").unwrap();
        assert!(user_pos < sep_pos);
    }

    #[test]
    fn empty_new_args_clears_user_args_keeps_internal() {
        let existing = vec![
            s("--base-path"), s("/data"),
            s("--chain"), s("/cfg/chain.json"),
            s("-lparachain=debug"),
        ];

        let result = merge_args(&existing, &[]);

        assert!(result.contains(&s("--base-path")));
        assert!(result.contains(&s("--chain")));
        assert!(!result.contains(&s("-lparachain=debug")));
    }
}