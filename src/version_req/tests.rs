use super::classify_bump;
use crate::model::Bump;

#[test]
fn classifies_cargo_compatibility_boundaries() {
    for (old, new, expected) in [
        ("1.2.3", "2.0.0", Bump::Major),
        ("1.2.3", "1.3.0", Bump::Minor),
        ("1.2.3", "1.2.4", Bump::Patch),
        ("0.1", "0.2", Bump::Major),
        ("0.1.1", "0.1.2", Bump::Patch),
        ("0.0.1", "0.0.2", Bump::Major),
        ("~0.29", "~0.30", Bump::Major),
        ("=0.0.1", "=0.0.2", Bump::Major),
        ("0.29.*", "0.30.*", Bump::Major),
        ("0.29", " ^0.29 ", Bump::Patch),
        ("*", "*", Bump::Patch),
        ("1.0.0-rc1", "1.0.0", Bump::Patch),
        ("1.0.0+build.7", "1.0.0", Bump::Patch),
    ] {
        assert_eq!(classify_bump(old, new), expected, "{old} -> {new}");
    }
}

#[test]
fn compares_compatibility_domains_before_requirement_spelling() {
    for (old, new) in [
        ("1", "2.0"),
        ("0.1", "0.2.0"),
        ("^1", "=2"),
        ("^0.0.1", "~0.0.2"),
        ("~0.0.2", "=0.0.1"),
    ] {
        assert_eq!(classify_bump(old, new), Bump::Major, "{old} -> {new}");
    }
}

#[test]
fn overlapping_domains_do_not_prove_a_major_change() {
    for (old, new) in [
        ("0.1", "0.1.1"),
        ("^0.0", "^0.0.1"),
        ("^1", "=1"),
        ("~0.0.1", "~0.0.2"),
        ("~0.0.1", "^0.0.2"),
    ] {
        assert_eq!(classify_bump(old, new), Bump::Unknown, "{old} -> {new}");
    }
}

#[test]
fn bounds_and_complex_changes_require_review() {
    for (old, new) in [
        ("<2", "<3"),
        ("<=0.29", "<=0.30"),
        (">=0.29, <0.31", ">=0.30, <0.32"),
        (">=0.29, <0.31", ">=0.29, <0.30"),
        (">=0.29", ">0.29"),
        ("^0.29", "~0.29"),
        ("^0", "^0.1"),
        ("*", "1"),
        ("1 | 2", "2 | 3"),
        ("not a requirement", "2"),
    ] {
        assert_eq!(classify_bump(old, new), Bump::Unknown, "{old} -> {new}");
    }
}

#[test]
fn prerelease_does_not_discard_following_bounds() {
    for (old, new) in [
        (">=1.0.0-alpha, <2", ">=1.0.0-alpha, <3"),
        (">=0.1.0-pre.1, <0.3", ">=0.2.0-pre.1, <0.4"),
        ("1.0.0-alpha", "1.0.0-beta"),
        ("1.0.0-alpha", "2.0.0-alpha"),
    ] {
        assert_eq!(classify_bump(old, new), Bump::Unknown, "{old} -> {new}");
    }
}
