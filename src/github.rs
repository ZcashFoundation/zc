//! GitHub Actions output: workflow-command annotations and the job step summary.
//!
//! Annotations go to stderr, so the stdout document of `--json` and `--changelog` stays
//! the only thing on stdout. The runner reads workflow commands from both streams.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;

use crate::model::{CrateStatus, FailOn, Report, Verdict};

/// True inside a GitHub Actions job.
pub fn is_active() -> bool {
    std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
}

pub fn emit(report: &Report, fail_on: FailOn) {
    for annotation in annotations(report, fail_on) {
        eprintln!("{annotation}");
    }
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(step_summary(report).as_bytes());
    }
}

/// Workflow-command lines describing the final result.
///
/// Severity follows the configured policy: a break that does not fail the run is a warning.
/// Dependency changes are always a warning; an analysis error is always an error.
pub fn annotations(report: &Report, fail_on: FailOn) -> Vec<String> {
    let mut lines = Vec::new();

    let api_breaks = report.api_breaking() + report.pubdep_break_total;
    if api_breaks > 0 {
        let level = if fail_on.fails_on_api_break() {
            "error"
        } else {
            "warning"
        };
        lines.push(format!(
            "::{level} title=Breaking public API change::{} breaking change(s) in {}",
            api_breaks,
            escape(&broken_crates(report).join(", "))
        ));
    }

    if report.deps.breaking > 0 {
        lines.push(format!(
            "::warning title=Consumer-visible dependency change::{}",
            escape(&breaking_deps(report).join(", "))
        ));
    }

    if report.verdict() == Verdict::Error {
        lines.push(format!(
            "::error title=API analysis failed::{}",
            escape(&failed_crates(report).join(", "))
        ));
    }

    lines
}

/// Markdown appended to `GITHUB_STEP_SUMMARY`.
pub fn step_summary(report: &Report) -> String {
    let mut out = format!("## zc: {}\n\n", report.verdict().as_str());
    let _ = writeln!(
        out,
        "`{}` ({}) -> `{}` ({})\n",
        report.refs.baseline_label,
        report.refs.baseline_short,
        report.refs.head_label,
        report.refs.head_short
    );

    out.push_str("| Total | Count |\n| --- | ---: |\n");
    for (label, count) in [
        ("API removed", report.removed_total),
        ("API changed", report.changed_total),
        ("API added", report.added_total),
        ("Breaking dependencies", report.deps.breaking),
        ("Public-dependency breaks", report.pubdep_break_total),
        ("Value changes", report.values.len()),
        ("Doc changes", report.docs.len()),
        ("Crates with analysis errors", report.error_crate_count),
    ] {
        let _ = writeln!(out, "| {label} | {count} |");
    }

    if report.changed_crate_count > 0 {
        out.push_str("\n| Crate | Removed | Changed | Added |\n| --- | ---: | ---: | ---: |\n");
        for result in report.crates.iter().filter(|result| result.total() > 0) {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                result.name, result.removed, result.changed, result.added
            );
        }
    }

    out
}

/// Crates carrying a public API or public-dependency break.
fn broken_crates(report: &Report) -> Vec<String> {
    report
        .crates
        .iter()
        .filter(|result| result.removed + result.changed > 0 || !result.pubdep.is_empty())
        .map(|result| result.name.clone())
        .collect()
}

/// The dependency changes counted by `deps.breaking`, with their versions.
fn breaking_deps(report: &Report) -> Vec<String> {
    let removed = report
        .deps
        .removed
        .iter()
        .filter(|dep| dep.breaking)
        .map(|dep| format!("{} {} removed", dep.name, dep.version));
    let changed = report
        .deps
        .changed
        .iter()
        .filter(|dep| dep.breaking)
        .map(|dep| format!("{} {} -> {}", dep.name, dep.old, dep.new));
    removed.chain(changed).collect()
}

fn failed_crates(report: &Report) -> Vec<String> {
    report
        .crates
        .iter()
        .filter(|result| result.status == CrateStatus::Error)
        .map(|result| match &result.error {
            Some(error) => format!("{} ({})", result.name, error.stage.as_str()),
            None => result.name.clone(),
        })
        .collect()
}

/// Workflow-command message escaping.
fn escape(message: &str) -> String {
    message
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
#[path = "github/tests.rs"]
mod tests;
