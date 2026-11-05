use std::collections::{BTreeMap, BTreeSet};

use super::{cargo_args, CrateFeatures, FeaturePolicy};
use crate::cargo_meta::CrateFeatureTable;

fn table(features: &[(&str, &[&str])]) -> CrateFeatureTable {
    CrateFeatureTable {
        features: features
            .iter()
            .map(|(name, entries)| {
                (
                    (*name).to_owned(),
                    entries.iter().map(|entry| (*entry).to_owned()).collect(),
                )
            })
            .collect(),
        optional_dependencies: BTreeSet::new(),
    }
}

fn exclude(names: &str) -> FeaturePolicy {
    FeaturePolicy::parse(&format!("exclude:{names}")).expect("valid exclude policy")
}

#[test]
fn parses_each_policy_and_rejects_everything_else() {
    assert_eq!(FeaturePolicy::parse("all"), Some(FeaturePolicy::All));
    assert_eq!(
        FeaturePolicy::parse("default"),
        Some(FeaturePolicy::Default)
    );
    assert_eq!(
        FeaturePolicy::parse("exclude:fuzzing, proptest-impl,fuzzing")
            .map(|policy| policy.to_string()),
        Some("exclude:fuzzing,proptest-impl".to_owned())
    );
    for invalid in ["", "none", "exclude:", "exclude: , ", "fuzzing", "ALL"] {
        assert_eq!(FeaturePolicy::parse(invalid), None, "{invalid:?}");
    }
}

#[test]
fn all_keeps_the_pre_policy_cache_fingerprint() {
    assert_eq!(FeaturePolicy::All.fingerprint_args(), ["--all-features"]);
    assert!(FeaturePolicy::Default.fingerprint_args().is_empty());
    assert_eq!(exclude("b,a").fingerprint_args(), ["exclude:a,b"]);
}

#[test]
fn all_and_default_ignore_the_feature_table() {
    let declared = table(&[("fuzzing", &[])]);
    assert_eq!(
        cargo_args(&FeaturePolicy::All, &declared),
        ["--all-features"]
    );
    assert!(cargo_args(&FeaturePolicy::Default, &declared).is_empty());
}

#[test]
fn exclude_enables_every_other_declared_feature_explicitly() {
    let declared = table(&[
        ("default", &["std"]),
        ("std", &[]),
        ("fuzzing", &[]),
        ("proptest-impl", &["dep:proptest"]),
    ]);
    assert_eq!(
        cargo_args(&exclude("fuzzing,proptest-impl,bench"), &declared),
        ["--no-default-features", "--features", "default,std"]
    );
}

#[test]
fn exclude_drops_features_that_enable_an_excluded_one() {
    let declared = table(&[
        ("default", &["extra"]),
        ("extra", &["fuzzing"]),
        ("fuzzing", &[]),
        ("via-dep", &["helper/std", "fuzzing"]),
        ("helper", &[]),
        ("unrelated", &["dep:other", "helper?/std"]),
    ]);
    assert_eq!(
        cargo_args(&exclude("fuzzing"), &declared),
        ["--no-default-features", "--features", "helper,unrelated"]
    );
}

#[test]
fn exclude_follows_the_implicit_feature_a_dependency_feature_enables() {
    let mut declared = table(&[
        ("tools", &["helper/std"]),
        ("helper", &["fuzzing"]),
        ("fuzzing", &[]),
    ]);
    declared.optional_dependencies.insert("helper".to_owned());
    assert_eq!(
        cargo_args(&exclude("fuzzing"), &declared),
        ["--no-default-features"]
    );
}

#[test]
fn exclude_survives_feature_cycles() {
    let declared = table(&[("a", &["b"]), ("b", &["a"])]);
    assert_eq!(
        cargo_args(&exclude("fuzzing"), &declared),
        ["--no-default-features", "--features", "a,b"]
    );
}

#[test]
fn required_dependency_feature_does_not_enable_same_name_local_feature() {
    let declared = table(&[
        ("tools", &["helper/std"]),
        ("helper", &["fuzzing"]),
        ("fuzzing", &[]),
    ]);
    assert_eq!(
        cargo_args(&exclude("fuzzing"), &declared),
        ["--no-default-features", "--features", "tools"]
    );
}

#[test]
fn weak_and_explicit_dependency_edges_do_not_enable_local_features() {
    let mut declared = table(&[
        ("weak", &["helper?/std"]),
        ("explicit", &["dep:helper"]),
        ("strong", &["helper/std"]),
        ("helper", &["fuzzing"]),
        ("fuzzing", &[]),
    ]);
    declared.optional_dependencies.insert("helper".to_owned());
    assert_eq!(
        cargo_args(&exclude("helper,fuzzing"), &declared),
        ["--no-default-features", "--features", "explicit,weak"]
    );
}

#[test]
fn cycles_reaching_excluded_features_are_removed_from_every_start() {
    let declared = table(&[
        ("a", &["b"]),
        ("b", &["a", "c"]),
        ("c", &["fuzzing"]),
        ("fuzzing", &[]),
        ("kept", &["other"]),
        ("other", &["kept"]),
    ]);
    assert_eq!(
        cargo_args(&exclude("fuzzing"), &declared),
        ["--no-default-features", "--features", "kept,other"]
    );
}

#[test]
fn feature_arguments_use_the_corresponding_ref_cache() {
    let features = CrateFeatures::new(exclude("fuzzing"));
    {
        let mut cache = features.lock();
        cache.insert(
            "base".to_owned(),
            BTreeMap::from([("pkg".to_owned(), table(&[("old", &[]), ("fuzzing", &[])]))]),
        );
        cache.insert(
            "head".to_owned(),
            BTreeMap::from([("pkg".to_owned(), table(&[("new", &[]), ("fuzzing", &[])]))]),
        );
    }
    let absent = std::path::Path::new("/not-a-worktree");
    assert_eq!(
        features.cargo_args("pkg", absent, "base").unwrap(),
        ["--no-default-features", "--features", "old"]
    );
    assert_eq!(
        features.cargo_args("pkg", absent, "head").unwrap(),
        ["--no-default-features", "--features", "new"]
    );
}
