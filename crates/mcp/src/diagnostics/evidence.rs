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

/// Recompute `summary` and `status` from the current evidence.
pub(super) fn finalize(report: &mut DiagnosticReport) {
    report.summary = summarize(&report.evidence);
    report.status = status_from_evidence(&report.evidence);
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
