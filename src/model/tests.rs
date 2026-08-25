use super::{
    CrateResult, CrateStatus, DepDiff, FailOn, Refs, Report, Verdict, EXIT_ANALYSIS, EXIT_BREAKING,
    EXIT_OK,
};

/// A report with nothing to report; each test sets the fields it is about.
fn report() -> Report {
    Report {
        refs: Refs {
            baseline: "base".to_string(),
            baseline_label: "main".to_string(),
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
        crate_count: 1,
        values: Vec::new(),
        docs: Vec::new(),
        removed_total: 0,
        changed_total: 0,
        added_total: 0,
        changed_crate_count: 0,
        error_crate_count: 0,
        pubdep_break_total: 0,
        pubdep_review_total: 0,
    }
}

fn failing_crate() -> CrateResult {
    CrateResult {
        name: "failing".to_string(),
        removed: 0,
        changed: 0,
        added: 0,
        removed_lines: Vec::new(),
        changed_lines: Vec::new(),
        added_lines: Vec::new(),
        status: CrateStatus::Error,
        error: None,
        pubdep: Vec::new(),
    }
}

#[test]
fn fail_on_parses_the_documented_values_only() {
    assert_eq!(FailOn::parse("breaking"), Some(FailOn::Breaking));
    assert_eq!(FailOn::parse("api-breaking"), Some(FailOn::ApiBreaking));
    assert_eq!(FailOn::parse("error"), Some(FailOn::Error));
    assert_eq!(FailOn::parse("none"), Some(FailOn::None));
    assert_eq!(FailOn::parse("api_breaking"), None);
    assert_eq!(FailOn::parse("Breaking"), None);
    assert_eq!(FailOn::parse(""), None);
}

#[test]
fn breaking_mode_keeps_the_verdict_exit_codes() {
    let clean = report();
    assert_eq!(clean.verdict(), Verdict::Ok);
    assert_eq!(FailOn::Breaking.exit_code(&clean), EXIT_OK);

    let mut api = report();
    api.removed_total = 1;
    assert_eq!(api.verdict(), Verdict::Breaking);
    assert_eq!(FailOn::Breaking.exit_code(&api), EXIT_BREAKING);

    let mut deps = report();
    deps.deps.breaking = 1;
    assert_eq!(FailOn::Breaking.exit_code(&deps), EXIT_BREAKING);

    let mut error = report();
    error.error_crate_count = 1;
    error.crates = vec![failing_crate()];
    assert_eq!(error.verdict(), Verdict::Error);
    assert_eq!(FailOn::Breaking.exit_code(&error), EXIT_ANALYSIS);
}

#[test]
fn api_breaking_mode_ignores_dependency_and_value_breakage() {
    let mut deps = report();
    deps.deps.breaking = 2;
    assert_eq!(deps.verdict(), Verdict::Breaking);
    assert_eq!(FailOn::ApiBreaking.exit_code(&deps), EXIT_OK);

    let mut values = report();
    values.values = vec![super::ValueChange {
        crate_name: "sample".to_string(),
        path: "sample::LIMIT".to_string(),
        ty: "usize".to_string(),
        old: "1".to_string(),
        new: "2".to_string(),
    }];
    assert_eq!(FailOn::ApiBreaking.exit_code(&values), EXIT_OK);

    let mut api = report();
    api.changed_total = 1;
    assert_eq!(FailOn::ApiBreaking.exit_code(&api), EXIT_BREAKING);

    let mut public_dep = report();
    public_dep.pubdep_break_total = 1;
    assert_eq!(FailOn::ApiBreaking.exit_code(&public_dep), EXIT_BREAKING);

    let mut error = report();
    error.error_crate_count = 1;
    error.crates = vec![failing_crate()];
    assert_eq!(FailOn::ApiBreaking.exit_code(&error), EXIT_ANALYSIS);
}

#[test]
fn error_mode_fails_only_on_an_inconclusive_analysis() {
    let mut api = report();
    api.removed_total = 3;
    api.pubdep_break_total = 1;
    api.deps.breaking = 1;
    assert_eq!(FailOn::Error.exit_code(&api), EXIT_OK);

    let mut error = report();
    error.error_crate_count = 1;
    error.crates = vec![failing_crate()];
    assert_eq!(FailOn::Error.exit_code(&error), EXIT_ANALYSIS);
}

#[test]
fn none_mode_never_fails() {
    let mut everything = report();
    everything.removed_total = 5;
    everything.pubdep_break_total = 2;
    everything.deps.breaking = 3;
    everything.error_crate_count = 1;
    everything.crates = vec![failing_crate()];
    assert_eq!(everything.verdict(), Verdict::Error);
    assert_eq!(FailOn::None.exit_code(&everything), EXIT_OK);
}
