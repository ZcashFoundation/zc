//! Machine-readable report serialization.

use serde::Serialize;

use crate::model::{ApiError, Bump, CrateResult, CrateStatus, Report};
use crate::version_req;

#[derive(Serialize)]
struct JsonReport<'a> {
    baseline: &'a str,
    baseline_sha: &'a str,
    head: &'a str,
    head_sha: &'a str,
    verdict: &'static str,
    totals: Totals,
    deps: Deps<'a>,
    values: Vec<Value<'a>>,
    docs: Vec<Doc<'a>>,
    public_dep_breaks: Vec<PublicDepBreak<'a>>,
    crates: Vec<Crate<'a>>,
}

#[derive(Serialize)]
struct Totals {
    removed: usize,
    changed: usize,
    added: usize,
    api_breaking: usize,
    dep_breaking: usize,
    error_crates: usize,
    value_changed: usize,
    doc_changed: usize,
    public_dep_breaking: usize,
}

#[derive(Serialize)]
struct Deps<'a> {
    removed: Vec<DepRemoved<'a>>,
    changed: Vec<DepChanged<'a>>,
    added: Vec<DepAdded<'a>>,
}

#[derive(Serialize)]
struct DepRemoved<'a> {
    name: &'a str,
    version: &'a str,
    kind: &'a str,
}

#[derive(Serialize)]
struct DepChanged<'a> {
    name: &'a str,
    old: &'a str,
    new: &'a str,
    bump: &'static str,
    kind: &'a str,
    features: &'a str,
}

#[derive(Serialize)]
struct DepAdded<'a> {
    name: &'a str,
    version: &'a str,
    kind: &'a str,
}

#[derive(Serialize)]
struct Value<'a> {
    #[serde(rename = "crate")]
    crate_name: &'a str,
    path: &'a str,
    #[serde(rename = "type")]
    ty: &'a str,
    old: &'a str,
    new: &'a str,
}

#[derive(Serialize)]
struct Doc<'a> {
    #[serde(rename = "crate")]
    crate_name: &'a str,
    path: &'a str,
}

#[derive(Serialize)]
struct PublicDepBreak<'a> {
    #[serde(rename = "crate")]
    crate_name: &'a str,
    dep: &'a str,
    old: &'a str,
    new: &'a str,
    class: &'static str,
}

#[derive(Serialize)]
struct Crate<'a> {
    name: &'a str,
    removed: usize,
    changed: usize,
    added: usize,
    status: &'static str,
    error: Option<Error<'a>>,
}

#[derive(Serialize)]
struct Error<'a> {
    stage: &'static str,
    #[serde(rename = "ref")]
    ref_label: &'a str,
    ref_sha: &'a str,
    command: &'a str,
    stderr: &'a str,
    hint: &'a str,
}

impl<'a> From<&'a ApiError> for Error<'a> {
    fn from(error: &'a ApiError) -> Self {
        Self {
            stage: error.stage.as_str(),
            ref_label: &error.ref_label,
            ref_sha: &error.ref_sha,
            command: &error.command,
            stderr: &error.stderr,
            hint: &error.hint,
        }
    }
}

impl<'a> From<&'a CrateResult> for Crate<'a> {
    fn from(result: &'a CrateResult) -> Self {
        Self {
            name: &result.name,
            removed: result.removed,
            changed: result.changed,
            added: result.added,
            status: match result.status {
                CrateStatus::Ok => "ok",
                CrateStatus::Error => "error",
            },
            error: result.error.as_ref().map(Error::from),
        }
    }
}

pub fn emit(report: &Report) -> String {
    let deps = Deps {
        removed: report
            .deps
            .removed
            .iter()
            .map(|dep| DepRemoved {
                name: &dep.name,
                version: &dep.version,
                kind: &dep.kind,
            })
            .collect(),
        changed: report
            .deps
            .changed
            .iter()
            .map(|dep| DepChanged {
                name: &dep.name,
                old: &dep.old,
                new: &dep.new,
                bump: dep.bump.as_str(),
                kind: &dep.kind,
                features: &dep.features,
            })
            .collect(),
        added: report
            .deps
            .added
            .iter()
            .map(|dep| DepAdded {
                name: &dep.name,
                version: &dep.version,
                kind: &dep.kind,
            })
            .collect(),
    };
    let values = report
        .values
        .iter()
        .map(|change| Value {
            crate_name: &change.crate_name,
            path: &change.path,
            ty: &change.ty,
            old: &change.old,
            new: &change.new,
        })
        .collect();
    let docs = report
        .docs
        .iter()
        .map(|change| Doc {
            crate_name: &change.crate_name,
            path: &change.path,
        })
        .collect();
    let public_dep_breaks = report
        .crates
        .iter()
        .flat_map(|result| {
            result.pubdep.iter().map(move |finding| PublicDepBreak {
                crate_name: &result.name,
                dep: &finding.dep,
                old: &finding.old,
                new: &finding.new,
                class: if version_req::classify_bump(&finding.old, &finding.new) == Bump::Major {
                    "breaking"
                } else {
                    "review"
                },
            })
        })
        .collect();
    let document = JsonReport {
        baseline: &report.refs.baseline_label,
        baseline_sha: &report.refs.baseline_short,
        head: &report.refs.head_label,
        head_sha: &report.refs.head_short,
        verdict: report.verdict().as_str(),
        totals: Totals {
            removed: report.removed_total,
            changed: report.changed_total,
            added: report.added_total,
            api_breaking: report.api_breaking(),
            dep_breaking: report.deps.breaking,
            error_crates: report.error_crate_count,
            value_changed: report.values.len(),
            doc_changed: report.docs.len(),
            public_dep_breaking: report.pubdep_break_total,
        },
        deps,
        values,
        docs,
        public_dep_breaks,
        crates: report.crates.iter().map(Crate::from).collect(),
    };
    serde_json::to_string_pretty(&document).unwrap_or_else(|_| "{}".to_string())
}
