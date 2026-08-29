use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Status {
    Ok,
    Warning,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Category {
    Startup,
    Liveness,
    Logs,
    Metrics,
    Rpc,
    Parachain,
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub severity: Severity,
    pub category: Category,
    pub subject: String,
    pub message: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub status: Status,
    pub summary: String,
    pub evidence: Vec<Evidence>,
    pub next_steps: Vec<String>,
}

impl DiagnosticReport {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            status: Status::Unknown,
            summary: summary.into(),
            evidence: Vec::new(),
            next_steps: Vec::new(),
        }
    }

    pub fn push_evidence(&mut self, evidence: Evidence) {
        self.evidence.push(evidence);
    }
}

pub fn status_from_evidence(evidence: &[Evidence]) -> Status {
    if evidence.iter().any(|item| item.severity == Severity::Error) {
        Status::Failed
    } else if evidence
        .iter()
        .any(|item| item.severity == Severity::Warning)
    {
        Status::Warning
    } else if evidence.iter().any(|item| item.severity == Severity::Info) {
        Status::Ok
    } else {
        Status::Unknown
    }
}

/// Trim `input` to the last `max_bytes` bytes on a UTF-8 char boundary.
pub fn bounded_tail_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }

    let start = input
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| input.len() - index <= max_bytes)
        .unwrap_or(input.len());

    input[start..].to_string()
}

/// Keep at most the last `max_lines` lines of `input`, then trim to `max_bytes`.
pub fn bounded_tail(input: &str, max_lines: usize, max_bytes: usize) -> String {
    let lines = input.lines().rev().take(max_lines).collect::<Vec<_>>();
    let joined = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
    bounded_tail_bytes(&joined, max_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(id: &str, severity: Severity) -> Evidence {
        Evidence {
            id: id.to_string(),
            severity,
            category: Category::Logs,
            subject: "alice".to_string(),
            message: "message".to_string(),
            source: "test".to_string(),
            excerpt: None,
        }
    }

    #[test]
    fn status_is_unknown_without_evidence() {
        assert_eq!(status_from_evidence(&[]), Status::Unknown);
    }

    #[test]
    fn status_is_failed_when_any_evidence_is_error() {
        let items = vec![
            evidence("a", Severity::Info),
            evidence("b", Severity::Error),
        ];

        assert_eq!(status_from_evidence(&items), Status::Failed);
    }

    #[test]
    fn bounded_tail_limits_lines() {
        let tail = bounded_tail("one\ntwo\nthree\nfour", 2, 1024);

        assert_eq!(tail, "three\nfour");
    }

    #[test]
    fn bounded_tail_limits_bytes() {
        let tail = bounded_tail("abcdef", 1, 3);

        assert_eq!(tail, "def");
    }

    #[test]
    fn bounded_tail_bytes_passes_through_short_input() {
        assert_eq!(bounded_tail_bytes("abc", 16), "abc");
    }

    #[test]
    fn bounded_tail_bytes_respects_char_boundary() {
        let input = "αβγδ";
        let tail = bounded_tail_bytes(input, 5);

        assert!(input.ends_with(&tail));
        assert!(tail.len() <= 5);
    }
}
