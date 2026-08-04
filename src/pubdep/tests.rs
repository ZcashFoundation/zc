use super::*;

fn row(crate_name: &str, dep: &str, req: &str, scope: Scope, pkg: &str) -> PerCrateDep {
    PerCrateDep {
        crate_name: crate_name.into(),
        dep: dep.into(),
        req: req.into(),
        scope,
        pkg: pkg.into(),
    }
}

#[test]
fn tables_skip_msrv_and_preserve_head_external_order() {
    let base = vec![
        row("demo", "renamed", "1", Scope::External, "real-package"),
        row("demo", "rust-version", "1.82", Scope::Msrv, ""),
    ];
    let head = vec![
        row("demo", "second", "2", Scope::External, "second"),
        row("demo", "renamed", "2", Scope::External, "real-package"),
        row("demo", "inside", "0.1", Scope::Internal, "inside"),
        row("demo", "rust-version", "1.83", Scope::Msrv, ""),
    ];

    let tables = PubdepTables::build(&base, &head);
    assert_eq!(tables.req(false, "demo", "renamed"), Some("1"));
    assert_eq!(tables.req(true, "demo", "inside"), Some("0.1"));
    assert_eq!(tables.req(true, "demo", "rust-version"), None);
    assert_eq!(tables.external["demo"], ["second", "renamed"]);
    assert_eq!(tables.packages["demo"]["renamed"], "real-package");
}

#[test]
fn reachability_matches_identifier_roots_and_cargo_renames() {
    let surface = concat!(
        "pub fn demo::one() -> renamed_dep::Type\n",
        "pub fn demo::two() -> real_package::Other\n",
        "pub fn demo::three() -> prefixrenamed_dep::Nope\n",
    );

    assert!(reachable(surface, "renamed-dep", "unrelated"));
    assert!(reachable(surface, "alias", "real-package"));
    assert!(!reachable(surface, "prefix", "missing"));
    assert!(!reachable("xrenamed_dep::Type", "renamed-dep", "missing"));
    assert!(reachable("renamed_dep::Type", "renamed-dep", "missing"));
}
