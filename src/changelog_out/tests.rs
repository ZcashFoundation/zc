use super::dependency_markdown;
use crate::model::{DepKind, PerCrateDep, Scope};

fn dep(req: &str, target: Option<&str>, kind: DepKind) -> PerCrateDep {
    PerCrateDep {
        crate_name: "alpha".to_string(),
        dep: "external".to_string(),
        req: req.to_string(),
        scope: Scope::External,
        pkg: "external".to_string(),
        target: target.map(str::to_owned),
        kind,
        optional: false,
    }
}

#[test]
fn target_specific_changes_do_not_replace_the_unconditional_requirement() {
    let base = vec![
        dep("1", None, DepKind::Runtime),
        dep("1", Some("cfg(windows)"), DepKind::Runtime),
    ];
    let head = vec![
        dep("1", None, DepKind::Runtime),
        dep("2", Some("cfg(windows)"), DepKind::Runtime),
    ];
    let markdown = dependency_markdown(&base, &head);
    let changes = &markdown.changed["alpha"];
    assert_eq!(changes.len(), 1);
    assert!(changes[0].contains("external 2"));
    assert!(changes[0].contains("cfg(windows)"));
    assert!(markdown.removed.is_empty());
}

#[test]
fn build_and_runtime_requirements_are_compared_independently() {
    let base = vec![
        dep("1", None, DepKind::Runtime),
        dep("1", None, DepKind::Build),
    ];
    let head = vec![
        dep("2", None, DepKind::Runtime),
        dep("3", None, DepKind::Build),
    ];
    let markdown = dependency_markdown(&base, &head);
    let changes = &markdown.changed["alpha"];
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().any(|line| line.contains("external 2")));
    assert!(changes
        .iter()
        .any(|line| line.contains("external 3") && line.contains("build")));
}

#[test]
fn dev_only_dependency_changes_do_not_create_release_entries() {
    for (base, head) in [
        (vec![], vec![dep("1", None, DepKind::Dev)]),
        (
            vec![dep("1", None, DepKind::Dev)],
            vec![dep("2", None, DepKind::Dev)],
        ),
        (vec![dep("1", None, DepKind::Dev)], vec![]),
    ] {
        let markdown = dependency_markdown(&base, &head);
        assert!(markdown.added.is_empty());
        assert!(markdown.changed.is_empty());
        assert!(markdown.removed.is_empty());
    }
}
