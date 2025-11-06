use configuration::types::Arg;

/// Parse args to extract those marked for removal (with `-:` prefix).
/// Returns a set of arg names/flags that should be removed from the final command.
///
/// # Examples
/// - `-:--insecure-validator-i-know-what-i-do` -> removes `--insecure-validator-i-know-what-i-do`
/// - `-:insecure-validator` -> removes `--insecure-validator` (normalized)
/// - `-:--prometheus-port` -> removes `--prometheus-port`
pub fn parse_removal_args(args: &[Arg]) -> Vec<String> {
    args.iter()
        .filter_map(|arg| match arg {
            Arg::Flag(flag) if flag.starts_with("-:") => {
                let mut flag_to_exclude = flag[2..].to_string();

                // Normalize flag format - ensure it starts with --
                if !flag_to_exclude.starts_with("--") {
                    flag_to_exclude = format!("--{flag_to_exclude}");
                }

                Some(flag_to_exclude)
            },
            _ => None,
        })
        .collect()
}

/// Apply arg removals to a vector of string arguments.
/// This filters out any args that match the removal list.
///
/// # Arguments
/// * `args` - The command arguments to filter
/// * `removals` - List of arg names/flags to remove
///
/// # Returns
/// Filtered vector with specified args removed
pub fn apply_arg_removals(args: Vec<String>, removals: &[String]) -> Vec<String> {
    if removals.is_empty() {
        return args;
    }

    let mut res = Vec::new();
    let mut skip_next = false;

    for (i, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        let should_remove = removals
            .iter()
            .any(|removal| arg == removal || arg.starts_with(&format!("{removal}=")));

        if should_remove {
            // Only skip next if this looks like an option (starts with --) and next arg doesn't start with --
            if !arg.contains("=") && i + 1 < args.len() {
                let next_arg = &args[i + 1];
                if !next_arg.starts_with("-") {
                    skip_next = true;
                }
            }
            continue;
        }

        res.push(arg.clone());
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_removal_args() {
        let args = vec![
            Arg::Flag("-:--insecure-validator-i-know-what-i-do".to_string()),
            Arg::Flag("--validator".to_string()),
            Arg::Flag("-:--no-telemetry".to_string()),
        ];

        let removals = parse_removal_args(&args);
        assert_eq!(removals.len(), 2);
        assert!(removals.contains(&"--insecure-validator-i-know-what-i-do".to_string()));
        assert!(removals.contains(&"--no-telemetry".to_string()));
    }

    #[test]
    fn test_apply_arg_removals_flag() {
        let args = vec![
            "--validator".to_string(),
            "--insecure-validator-i-know-what-i-do".to_string(),
            "--no-telemetry".to_string(),
        ];
        let removals = vec!["--insecure-validator-i-know-what-i-do".to_string()];
        let res = apply_arg_removals(args, &removals);
        assert_eq!(res.len(), 2);
        assert!(res.contains(&"--validator".to_string()));
        assert!(res.contains(&"--no-telemetry".to_string()));
        assert!(!res.contains(&"--insecure-validator-i-know-what-i-do".to_string()));
    }

    #[test]
    fn test_apply_arg_removals_option_with_equals() {
        let args = vec!["--name=alice".to_string(), "--port=30333".to_string()];
        let removals = vec!["--port".to_string()];
        let res = apply_arg_removals(args, &removals);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], "--name=alice");
    }

    #[test]
    fn test_apply_arg_removals_option_with_space() {
        let args = vec![
            "--name".to_string(),
            "alice".to_string(),
            "--port".to_string(),
            "30333".to_string(),
        ];
        let removals = vec!["--port".to_string()];

        let res = apply_arg_removals(args, &removals);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], "--name");
        assert_eq!(res[1], "alice");
    }

    #[test]
    fn test_apply_arg_removals_empty() {
        let args = vec!["--validator".to_string()];
        let removals = vec![];

        let res = apply_arg_removals(args, &removals);
        assert_eq!(res, vec!["--validator".to_string()]);
    }
}
