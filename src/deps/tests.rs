use std::collections::BTreeMap;

use super::{diff, feature_diff};
use crate::model::{DepKind, DepRecord};

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

    let result = diff(&base, &head);

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

    let result = diff(&base, &head);

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

    let result = diff(&base, &head);

    assert_eq!(result.changed[0].kind, "runtime");
    assert_eq!(result.changed[1].kind, "runtime-opt");
    assert_eq!(result.breaking, 1);
}
