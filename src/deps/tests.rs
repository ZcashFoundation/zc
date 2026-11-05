use std::collections::BTreeMap;

use super::{diff, feature_diff};
use crate::model::{Bump, DepKind, DepRecord, PerCrateDep, Scope};

fn dep(
    ver: &str,
    kind: DepKind,
    optional: bool,
    default_features: bool,
    features: &[&str],
) -> DepRecord {
    DepRecord {
        ver: ver.to_string(),
        kind,
        optional,
        default_features,
        features: features
            .iter()
            .map(|feature| (*feature).to_string())
            .collect(),
    }
}

fn row(name: &str, req: &str, kind: DepKind, optional: bool) -> PerCrateDep {
    PerCrateDep {
        crate_name: "member".into(),
        dep: name.into(),
        req: req.into(),
        scope: Scope::External,
        pkg: name.into(),
        target: None,
        kind,
        optional,
    }
}

#[test]
fn feature_delta_puts_default_first_and_sorts_set_difference() {
    let old = vec!["zeta".to_string(), "foo".to_string(), "foo".to_string()];
    let new = vec!["async-std".to_string(), "alpha".to_string()];

    assert_eq!(
        feature_diff(true, false, &old, &new),
        "-default!,+alpha,+async-std,-foo,-zeta"
    );
}

#[test]
fn added_hyphenated_feature_is_not_a_removed_feature() {
    let base = BTreeMap::from([(
        "dep".to_string(),
        dep("1.0", DepKind::Runtime, false, true, &[]),
    )]);
    let head = BTreeMap::from([(
        "dep".to_string(),
        dep("1.1", DepKind::Runtime, false, true, &["async-std"]),
    )]);

    let result = diff(
        &base,
        &head,
        &[row("dep", "1.0", DepKind::Runtime, false)],
        &[row("dep", "1.1", DepKind::Runtime, false)],
    );

    assert_eq!(result.changed[0].features, "+async-std");
    assert_eq!(result.breaking, 0);
}

#[test]
fn breaking_count_is_limited_to_non_optional_runtime_breakage() {
    let base = BTreeMap::from([
        (
            "removed-runtime".to_string(),
            dep("1", DepKind::Runtime, false, true, &[]),
        ),
        (
            "removed-optional".to_string(),
            dep("1", DepKind::Runtime, true, true, &[]),
        ),
        (
            "major-runtime".to_string(),
            dep("1", DepKind::Runtime, false, true, &[]),
        ),
        (
            "major-build".to_string(),
            dep("1", DepKind::Build, false, true, &[]),
        ),
        (
            "lost-feature".to_string(),
            dep("1", DepKind::Runtime, false, true, &["std"]),
        ),
        (
            "lost-default".to_string(),
            dep("1", DepKind::Runtime, false, true, &[]),
        ),
        (
            "added-feature".to_string(),
            dep("1", DepKind::Runtime, false, true, &[]),
        ),
    ]);
    let head = BTreeMap::from([
        (
            "major-runtime".to_string(),
            dep("2", DepKind::Runtime, false, true, &[]),
        ),
        (
            "major-build".to_string(),
            dep("2", DepKind::Build, false, true, &[]),
        ),
        (
            "lost-feature".to_string(),
            dep("1", DepKind::Runtime, false, true, &[]),
        ),
        (
            "lost-default".to_string(),
            dep("1", DepKind::Runtime, false, false, &[]),
        ),
        (
            "added-feature".to_string(),
            dep("1", DepKind::Runtime, false, true, &["foo-bar"]),
        ),
    ]);

    let result = diff(
        &base,
        &head,
        &[
            row("major-runtime", "1", DepKind::Runtime, false),
            row("major-build", "1", DepKind::Build, false),
        ],
        &[
            row("major-runtime", "2", DepKind::Runtime, false),
            row("major-build", "2", DepKind::Build, false),
        ],
    );

    assert_eq!(result.breaking, 4);
}

#[test]
fn stronger_kind_wins_and_kind_ties_use_head_optionality() {
    let base = BTreeMap::from([
        (
            "stronger-old".to_string(),
            dep("1", DepKind::Runtime, false, true, &[]),
        ),
        (
            "tie".to_string(),
            dep("1", DepKind::Runtime, false, true, &[]),
        ),
    ]);
    let head = BTreeMap::from([
        (
            "stronger-old".to_string(),
            dep("1", DepKind::Build, false, false, &[]),
        ),
        (
            "tie".to_string(),
            dep("2", DepKind::Runtime, true, true, &[]),
        ),
    ]);

    let result = diff(
        &base,
        &head,
        &[
            row("stronger-old", "1", DepKind::Runtime, false),
            row("tie", "1", DepKind::Runtime, false),
        ],
        &[
            row("stronger-old", "1", DepKind::Build, false),
            row("tie", "2", DepKind::Runtime, true),
        ],
    );

    assert_eq!(result.changed[0].kind, "runtime");
    assert_eq!(result.changed[1].kind, "runtime-opt");
    assert_eq!(result.breaking, 1);
}

#[test]
fn required_member_migration_is_not_hidden_by_another_members_requirement() {
    let base = BTreeMap::from([(
        "dep".into(),
        dep("1 | 2", DepKind::Runtime, false, true, &[]),
    )]);
    let head = BTreeMap::from([(
        "dep".into(),
        dep("2 | 3.0", DepKind::Runtime, false, true, &[]),
    )]);
    let mut retained = row("dep", "2", DepKind::Runtime, false);
    retained.crate_name = "other".into();
    let base_rows = [row("dep", "1", DepKind::Runtime, false), retained.clone()];
    let head_rows = [row("dep", "3.0", DepKind::Runtime, false), retained];

    let result = diff(&base, &head, &base_rows, &head_rows);
    assert_eq!(result.breaking, 1);
    assert_eq!(result.changed[0].bump, Bump::Major);
    assert_eq!(result.changed[0].old, "1 | 2");
    assert_eq!(result.changed[0].new, "2 | 3.0");
}

#[test]
fn non_runtime_migrations_do_not_borrow_another_rows_required_usage() {
    for (kind, optional) in [
        (DepKind::Runtime, true),
        (DepKind::Build, false),
        (DepKind::Dev, false),
    ] {
        let base = BTreeMap::from([(
            "dep".into(),
            dep("1 | 2", DepKind::Runtime, false, true, &[]),
        )]);
        let head = BTreeMap::from([(
            "dep".into(),
            dep("2 | 3", DepKind::Runtime, false, true, &[]),
        )]);
        let mut retained = row("dep", "2", DepKind::Runtime, false);
        retained.crate_name = "other".into();
        let base_rows = [row("dep", "1", kind, optional), retained.clone()];
        let head_rows = [row("dep", "3", kind, optional), retained];

        let result = diff(&base, &head, &base_rows, &head_rows);
        assert_eq!(result.changed[0].bump, Bump::Major);
        assert!(!result.changed[0].breaking);
        assert_eq!(result.breaking, 0);
    }
}

#[test]
fn target_migration_is_classified_even_when_aggregate_requirements_do_not_change() {
    let records = BTreeMap::from([(
        "dep".into(),
        dep("1 | 2", DepKind::Runtime, false, true, &[]),
    )]);
    let mut windows_old = row("dep", "1", DepKind::Runtime, false);
    windows_old.target = Some("cfg(windows)".into());
    let mut windows_new = windows_old.clone();
    windows_new.req = "2".into();
    let mut unix_old = row("dep", "2", DepKind::Runtime, false);
    unix_old.target = Some("cfg(unix)".into());
    let mut unix_new = unix_old.clone();
    unix_new.req = "1".into();

    let result = diff(
        &records,
        &records,
        &[windows_old, unix_old],
        &[windows_new, unix_new],
    );
    assert_eq!(result.changed.len(), 1);
    assert_eq!(result.changed[0].bump, Bump::Major);
    assert_eq!(result.breaking, 1);
}

#[test]
fn unmatched_rows_and_ambiguous_ranges_do_not_invent_major_migrations() {
    let base = BTreeMap::from([("dep".into(), dep("1", DepKind::Runtime, false, true, &[]))]);
    for (base_row, head_row) in [
        (
            row("dep", "1", DepKind::Runtime, false),
            PerCrateDep {
                crate_name: "other".into(),
                ..row("dep", "2", DepKind::Runtime, false)
            },
        ),
        (
            row("dep", "1", DepKind::Runtime, false),
            PerCrateDep {
                target: Some("cfg(windows)".into()),
                ..row("dep", "2", DepKind::Runtime, false)
            },
        ),
        (
            row("dep", "1", DepKind::Runtime, false),
            row("dep", "2", DepKind::Build, false),
        ),
        (
            row("dep", ">=1, <4", DepKind::Runtime, false),
            row("dep", ">=2, <5", DepKind::Runtime, false),
        ),
    ] {
        let head = BTreeMap::from([(
            "dep".into(),
            dep(&head_row.req, DepKind::Runtime, false, true, &[]),
        )]);
        let result = diff(&base, &head, &[base_row], &[head_row]);
        assert_eq!(result.changed[0].bump, Bump::Unknown);
        assert_eq!(result.breaking, 0);
    }
}
