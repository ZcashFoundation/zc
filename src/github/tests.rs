use super::annotations;
use crate::model::{
    ApiError, Bump, CrateChange, CrateResult, CrateStatus, DepChanged, DepDiff, DepRemoved,
    ErrorStage, FailOn, PubdepFinding, Refs, Report,
};

fn report() -> Report {
    Report {
        refs: Refs {
            baseline: "base".to_string(),
            baseline_label: "merge-base(main, HEAD)".to_string(),
            baseline_sha: "1".repeat(40),
            baseline_short: "1111111".to_string(),
            head_ref: "HEAD".to_string(),
            head_label: "HEAD".to_string(),
            head_sha: "2".repeat(40),
            head_short: "2222222".to_string(),
            head_is_worktree_snapshot: false,
        },
        deps: DepDiff::default(),
        crates: Vec::new(),
        crate_count: 2,
        skipped_crates: Vec::new(),
        lock: None,
        values: Vec::new(),
        docs: Vec::new(),
        removed_total: 0,
        changed_total: 0,
        added_total: 0,
        changed_crate_count: 0,
        error_crate_count: 0,
        pubdep_break_total: 0,
        pubdep_review_total: 0,
        features: crate::features::FeaturePolicy::All,
    }
}

fn crate_result(name: &str, removed: usize, changed: usize, added: usize) -> CrateResult {
    CrateResult {
        name: name.to_string(),
        change: CrateChange::Existing,
        lib_name: name.replace('-', "_"),
        breaking_added: Vec::new(),
        removed,
        changed,
        added,
        removed_lines: Vec::new(),
        changed_lines: Vec::new(),
        added_lines: Vec::new(),
        status: CrateStatus::Ok,
        error: None,
        pubdep: Vec::new(),
    }
}

/// Two crates break the public API, one of them through a public dependency.
fn breaking_report() -> Report {
    let mut report = report();
    let mut second = crate_result("zebra-state", 0, 0, 0);
    second.pubdep = vec![PubdepFinding {
        dep: "rocksdb".to_string(),
        old_package: "rocksdb".to_string(),
        new_package: "rocksdb".to_string(),
        old: "0.21".to_string(),
        new: "0.22".to_string(),
        target: None,
    }];
    report.crates = vec![crate_result("zebra-chain", 2, 1, 4), second];
    report.removed_total = 2;
    report.changed_total = 1;
    report.added_total = 4;
    report.changed_crate_count = 1;
    report.pubdep_break_total = 1;
    report
}

#[test]
fn a_break_that_does_not_fail_the_run_is_a_warning() {
    let report = breaking_report();
    for mode in [FailOn::Error, FailOn::None] {
        assert!(annotations(&report, mode)[0].starts_with("::warning title=Breaking public API"));
    }
    assert!(annotations(&report, FailOn::ApiBreaking)[0]
        .starts_with("::error title=Breaking public API"));
}

#[test]
fn dependency_changes_stay_warnings_and_list_the_counted_deps() {
    let mut report = report();
    report.deps = DepDiff {
        removed: vec![DepRemoved {
            name: "gone".to_string(),
            version: "1.2".to_string(),
            kind: "runtime".to_string(),
            breaking: true,
        }],
        changed: vec![
            DepChanged {
                name: "tokio".to_string(),
                old: "1.0".to_string(),
                new: "2.0".to_string(),
                bump: Bump::Major,
                kind: "runtime".to_string(),
                features: String::new(),
                breaking: true,
            },
            DepChanged {
                name: "serde".to_string(),
                old: "1.0".to_string(),
                new: "1.1".to_string(),
                bump: Bump::Minor,
                kind: "runtime".to_string(),
                features: String::new(),
                breaking: false,
            },
        ],
        added: Vec::new(),
        breaking: 2,
    };

    assert_eq!(
        annotations(&report, FailOn::Breaking),
        vec![
            "::warning title=Consumer-visible dependency change::gone 1.2 removed, \
             tokio 1.0 -> 2.0"
                .to_string()
        ]
    );
}

#[test]
fn an_analysis_failure_names_the_crate_and_its_stage() {
    let mut report = report();
    let mut failing = crate_result("zebra-rpc", 0, 0, 0);
    failing.status = CrateStatus::Error;
    failing.error = Some(ApiError {
        stage: ErrorStage::HeadBuild,
        ref_label: "HEAD".to_string(),
        ref_sha: "2".repeat(40),
        command: "cargo public-api".to_string(),
        stderr: String::new(),
        hint: "hint".to_string(),
    });
    report.crates = vec![failing];
    report.error_crate_count = 1;

    assert_eq!(
        annotations(&report, FailOn::None),
        vec!["::error title=API analysis failed::zebra-rpc (head_build)".to_string()]
    );
}

#[test]
fn one_sided_failures_retain_lifecycle_context_without_successful_changes() {
    let mut report = report();
    for (name, change, stage) in [
        ("newborn", CrateChange::Added, ErrorStage::HeadBuild),
        ("vanished", CrateChange::Removed, ErrorStage::BaselineBuild),
    ] {
        let mut failing = crate_result(name, 0, 0, 0);
        failing.change = change;
        failing.status = CrateStatus::Error;
        failing.error = Some(ApiError {
            stage,
            ref_label: "ref".to_string(),
            ref_sha: "2".repeat(40),
            command: "cargo public-api".to_string(),
            stderr: String::new(),
            hint: "hint".to_string(),
        });
        report.crates.push(failing);
    }
    report.error_crate_count = report.crates.len();
    let messages = annotations(&report, FailOn::None).join("\n");
    let summary = super::step_summary(&report);
    for context in [
        "newborn (crate added) (head_build)",
        "vanished (crate removed) (baseline_build)",
    ] {
        assert!(messages.contains(context), "{messages}");
        assert!(summary.contains(context), "{summary}");
    }
}

#[test]
fn a_clean_report_is_not_annotated() {
    assert!(annotations(&report(), FailOn::Breaking).is_empty());
}

#[test]
fn message_data_is_escaped() {
    let mut report = report();
    report.deps = DepDiff {
        removed: vec![DepRemoved {
            name: "odd%name".to_string(),
            version: "1.0".to_string(),
            kind: "runtime".to_string(),
            breaking: true,
        }],
        changed: Vec::new(),
        added: Vec::new(),
        breaking: 1,
    };

    assert!(annotations(&report, FailOn::Breaking)[0].ends_with("odd%25name 1.0 removed"));
}
