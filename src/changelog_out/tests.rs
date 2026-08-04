use super::dependency_markdown;
use crate::model::{PerCrateDep, Scope};

fn dep(crate_name: &str, name: &str, req: &str, scope: Scope) -> PerCrateDep {
    PerCrateDep {
        crate_name: crate_name.to_string(),
        dep: name.to_string(),
        req: req.to_string(),
        scope,
        pkg: name.to_string(),
    }
}

#[test]
fn dependency_markdown_matches_changelog_wording_and_order() {
    let base = vec![
        dep("alpha", "external", "1", Scope::External),
        dep("alpha", "internal", "1", Scope::Internal),
        dep("alpha", "removed", "3", Scope::External),
        dep("alpha", "~msrv", "1.75", Scope::Msrv),
        dep("beta", "~msrv", "1.70", Scope::Msrv),
    ];
    let head = vec![
        dep("alpha", "external", "2", Scope::External),
        dep("alpha", "internal", "2", Scope::Internal),
        dep("alpha", "~msrv", "1.81", Scope::Msrv),
        dep("beta", "~msrv", "-", Scope::Msrv),
    ];

    let markdown = dependency_markdown(&base, &head);
    assert_eq!(
        markdown.changed.get("alpha"),
        Some(&vec![
            "- MSRV is now 1.81.".to_string(),
            "- Migrated to `external 2`.".to_string(),
            "- `internal` dependency bumped to `2`.".to_string(),
        ])
    );
    assert_eq!(
        markdown.removed.get("alpha"),
        Some(&vec!["- `removed` dependency.".to_string()])
    );
    assert!(!markdown.removed.contains_key("beta"));
    assert!(!markdown.changed.contains_key("beta"));
}
