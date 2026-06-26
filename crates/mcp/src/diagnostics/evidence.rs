use std::{collections::BTreeMap, path::Path};

use crate::report::{
    bounded_tail, status_from_evidence, Category, DiagnosticReport, Evidence, Severity,
};

const EXCERPT_MAX_BYTES: usize = 8 * 1024;
const EXCERPT_MAX_LINES: usize = 20;

/// Append `Evidence` to `report`, trimming the optional `excerpt` to a safe size.
#[allow(clippy::too_many_arguments)]
pub(super) fn push(
    report: &mut DiagnosticReport,
    severity: Severity,
    id: impl Into<String>,
    category: Category,
    subject: impl Into<String>,
    message: impl Into<String>,
    source: impl Into<String>,
    excerpt: Option<String>,
) {
    report.push_evidence(Evidence {
        id: id.into(),
        severity,
        category,
        subject: subject.into(),
        message: message.into(),
        source: source.into(),
        excerpt: excerpt.map(|text| bounded_tail(&text, EXCERPT_MAX_LINES, EXCERPT_MAX_BYTES)),
    });
}

/// Record an `input.invalid` error if `result` is `Err`. Returns `true` when input is valid.
pub(super) fn validate_input<E: std::fmt::Display>(
    report: &mut DiagnosticReport,
    result: Result<(), E>,
    subject: impl Into<String>,
    source: &Path,
    message: &'static str,
) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            push(
                report,
                Severity::Error,
                "input.invalid",
                Category::Config,
                subject,
                message,
                source.display().to_string(),
                Some(error.to_string()),
            );
            false
        },
    }
}

/// Collapse repeated findings, then recompute `summary` and `status`.
pub(super) fn finalize(report: &mut DiagnosticReport) {
    dedupe(&mut report.evidence);
    report.summary = summarize(&report.evidence);
    report.status = status_from_evidence(&report.evidence);
}

fn dedupe(evidence: &mut Vec<Evidence>) {
    let mut deduped: Vec<Evidence> = Vec::with_capacity(evidence.len());
    for item in evidence.drain(..) {
        if !deduped.contains(&item) {
            deduped.push(item);
        }
    }
    *evidence = deduped;
}

pub(super) fn summarize(evidence: &[Evidence]) -> String {
    if evidence.is_empty() {
        return "No startup diagnostics were found".to_string();
    }

    let mut counts: BTreeMap<Severity, usize> = BTreeMap::new();
    for item in evidence {
        *counts.entry(item.severity).or_default() += 1;
    }

    counts
        .into_iter()
        .map(|(severity, count)| format!("{severity:?}: {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_collapses_repeated_findings() {
        let mut report = DiagnosticReport::new("test");
        for _ in 0..3 {
            push(
                &mut report,
                Severity::Error,
                "logs.unexpected argument",
                Category::Startup,
                "alice.log",
                "A node command-line argument appears to be invalid",
                "alice.log",
                Some("error: unexpected argument '--bad' found".to_string()),
            );
        }
        push(
            &mut report,
            Severity::Warning,
            "zombie_json.missing",
            Category::Startup,
            "zombie.json",
            "zombie.json file was not found",
            "zombie.json",
            None,
        );

        finalize(&mut report);

        assert_eq!(report.evidence.len(), 2);
        assert!(report
            .evidence
            .iter()
            .any(|e| e.id == "logs.unexpected argument"));
        assert_eq!(report.summary, "Warning: 1, Error: 1");
    }
}
