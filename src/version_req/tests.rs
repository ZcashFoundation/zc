use super::{classify_bump, req_norm, req_version};
use crate::model::Bump;

#[test]
fn normalizes_caret_and_metadata() {
    assert_eq!(req_norm(" ^1.0.0 "), "1.0.0");
    assert_eq!(req_norm("1.0.0-rc1"), "1.0.0");
    assert_eq!(req_norm("1.0.0+build.7"), "1.0.0");
    assert_eq!(req_norm("1.0.0-alpha-beta"), "1.0.0");
}

#[test]
fn extracts_first_version_literal() {
    assert_eq!(req_version("^1.2.3"), "1.2.3");
    assert_eq!(req_version("~0.29"), "0.29");
    assert_eq!(req_version("=0.0.1"), "0.0.1");
    assert_eq!(req_version(">=0.29, <0.31"), "0.29");
    assert_eq!(req_version("0.29.*"), "0.29");
    assert_eq!(req_version("*"), "");
}

#[test]
fn prerelease_graduation_is_not_a_requirement_change() {
    assert_eq!(req_norm("1.0.0-rc1"), req_norm("1.0.0"));
    assert_eq!(classify_bump("1.0.0-rc1", "1.0.0"), Bump::Patch);
}

#[test]
fn classifies_caret_compatibility_boundaries() {
    // Cargo's caret rules: the leftmost nonzero component decides compatibility.
    assert_eq!(classify_bump("1.2.3", "2.0.0"), Bump::Major);
    assert_eq!(classify_bump("1.2.3", "1.3.0"), Bump::Minor);
    assert_eq!(classify_bump("1.2.3", "1.2.4"), Bump::Patch);
    assert_eq!(classify_bump("0.1", "0.2"), Bump::Major);
    assert_eq!(classify_bump("0.1.1", "0.1.2"), Bump::Patch);
    assert_eq!(classify_bump("0.29", "0.30"), Bump::Major);
    assert_eq!(classify_bump("0.0.1", "0.0.2"), Bump::Major);
    assert_eq!(classify_bump("0.0.1", "0.0.1"), Bump::Patch);
}

#[test]
fn operators_and_wildcards_reduce_to_the_version_they_start_at() {
    assert_eq!(classify_bump("~0.29", "~0.30"), Bump::Major);
    assert_eq!(classify_bump("=0.0.1", "=0.0.2"), Bump::Major);
    assert_eq!(classify_bump("0.30.0-pre.0", "0.30.0"), Bump::Patch);
    // A compound range is represented by its floor, so a moved floor still classifies.
    assert_eq!(classify_bump(">=0.29, <0.31", ">=0.30, <0.32"), Bump::Major);
    // A bare requirement and a caret requirement are the same set: no change.
    assert_eq!(classify_bump("0.29", "^0.29"), Bump::Patch);
    // A wildcard reduces to its numeric prefix; a bare `*` names no version at all.
    assert_eq!(classify_bump("0.29.*", "0.30.*"), Bump::Major);
    assert_eq!(classify_bump("*", "*"), Bump::Patch);
}

#[test]
fn unchanged_floor_with_changed_text_is_unknown() {
    // With the floor held the change is confined to the ceiling or an operator, and the
    // requirement text alone does not say whether a consumer is affected.
    assert_eq!(
        classify_bump(">=0.29, <0.31", ">=0.29, <0.30"),
        Bump::Unknown
    );
    assert_eq!(
        classify_bump(">=0.29, <0.31", ">=0.29, <0.32"),
        Bump::Unknown
    );
    assert_eq!(classify_bump(">=0.29", ">0.29"), Bump::Unknown);
    assert_eq!(classify_bump("^0.29", "~0.29"), Bump::Unknown);
}
