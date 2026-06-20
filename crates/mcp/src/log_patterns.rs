pub use crate::report::{Category, Severity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogMatch {
    pub severity: Severity,
    pub category: Category,
    pub pattern: &'static str,
    pub message: &'static str,
    pub line: String,
}

#[derive(Debug, Clone)]
struct LogPattern {
    severity: Severity,
    category: Category,
    pattern: &'static str,
    match_text: &'static str,
    message: &'static str,
}

static LOG_PATTERNS: &[LogPattern] = &[
    LogPattern {
        severity: Severity::Error,
        category: Category::Startup,
        pattern: "address already in use",
        match_text: "address already in use",
        message: "A port appears to be busy",
    },
    LogPattern {
        severity: Severity::Error,
        category: Category::Startup,
        pattern: "No such file or directory",
        match_text: "no such file or directory",
        message: "A required binary or file is missing",
    },
    LogPattern {
        severity: Severity::Error,
        category: Category::Startup,
        pattern: "permission denied",
        match_text: "permission denied",
        message: "A permission problem prevented startup",
    },
    LogPattern {
        severity: Severity::Error,
        category: Category::Startup,
        pattern: "unexpected argument",
        match_text: "unexpected argument",
        message: "A node command-line argument appears to be invalid",
    },
    LogPattern {
        severity: Severity::Error,
        category: Category::Logs,
        pattern: "panicked at",
        match_text: "panicked at",
        message: "The node process panicked",
    },
    LogPattern {
        severity: Severity::Error,
        category: Category::Logs,
        pattern: "Essential task",
        match_text: "essential task",
        message: "An essential node task failed",
    },
    LogPattern {
        severity: Severity::Error,
        category: Category::Parachain,
        pattern: "Failed to register parachain",
        match_text: "failed to register parachain",
        message: "Parachain registration failed",
    },
    LogPattern {
        severity: Severity::Warning,
        category: Category::Rpc,
        pattern: "Connection refused",
        match_text: "connection refused",
        message: "A local endpoint refused a connection",
    },
];

pub fn scan_logs(logs: &str) -> Vec<LogMatch> {
    logs.lines()
        .flat_map(|line| {
            let line_lower = line.to_lowercase();

            LOG_PATTERNS
                .iter()
                .filter(move |pattern| line_lower.contains(pattern.match_text))
                .map(move |pattern| LogMatch {
                    severity: pattern.severity,
                    category: pattern.category,
                    pattern: pattern.pattern,
                    message: pattern.message,
                    line: line.to_string(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_port_conflict() {
        let line = "Error: listen tcp 127.0.0.1:9944: bind: address already in use";
        let matches = scan_logs(line);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].severity, Severity::Error);
        assert_eq!(matches[0].category, Category::Startup);
        assert_eq!(matches[0].pattern, "address already in use");
        assert_eq!(matches[0].message, "A port appears to be busy");
        assert_eq!(matches[0].line, line);
    }

    #[test]
    fn detects_parachain_registration_failure() {
        let line = "Failed to register parachain 2000 with relay chain";
        let logs = format!("node booted\n{line}");

        let matches = scan_logs(&logs);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].severity, Severity::Error);
        assert_eq!(matches[0].category, Category::Parachain);
        assert_eq!(matches[0].pattern, "Failed to register parachain");
        assert_eq!(matches[0].message, "Parachain registration failed");
        assert_eq!(matches[0].line, line);
    }

    #[test]
    fn detects_unexpected_argument_startup_failure() {
        let line = "error: unexpected argument '--zombienet-mcp-demo-invalid-flag' found";
        let matches = scan_logs(line);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].severity, Severity::Error);
        assert_eq!(matches[0].category, Category::Startup);
        assert_eq!(matches[0].pattern, "unexpected argument");
        assert_eq!(
            matches[0].message,
            "A node command-line argument appears to be invalid"
        );
        assert_eq!(matches[0].line, line);
    }

    #[test]
    fn pattern_messages_match_reviewed_contract() {
        let logs = [
            "Error: listen tcp 127.0.0.1:9944: bind: address already in use",
            "No such file or directory: polkadot",
            "permission denied: ./polkadot",
            "error: unexpected argument '--zombienet-mcp-demo-invalid-flag' found",
            "thread 'main' panicked at runtime",
            "Essential task `alice` failed. Shutting down service.",
            "Failed to register parachain 2000",
            "Connection refused (os error 61)",
        ]
        .join("\n");

        let messages: Vec<_> = scan_logs(&logs)
            .into_iter()
            .map(|log_match| log_match.message)
            .collect();

        assert_eq!(
            messages,
            [
                "A port appears to be busy",
                "A required binary or file is missing",
                "A permission problem prevented startup",
                "A node command-line argument appears to be invalid",
                "The node process panicked",
                "An essential node task failed",
                "Parachain registration failed",
                "A local endpoint refused a connection",
            ]
        );
    }

    #[test]
    fn scan_logs_handles_unrelated_input() {
        assert!(scan_logs("ordinary log line\nunicode αβγ").is_empty());
    }
}
