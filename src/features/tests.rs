use std::collections::BTreeMap;

use super::{cargo_args, FeaturePolicy};

fn table(features: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    features
        .iter()
        .map(|(name, entries)| {
            (
                (*name).to_owned(),
                entries.iter().map(|entry| (*entry).to_owned()).collect(),
            )
        })
        .collect()
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
    let declared = table(&[
        ("tools", &["helper/std"]),
        ("helper", &["fuzzing"]),
        ("fuzzing", &[]),
    ]);
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
