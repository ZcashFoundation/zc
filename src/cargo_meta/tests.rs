use super::{classify_api_crates, ApiCrates, Metadata};
use crate::model::{SkipReason, SkippedCrate};

/// `cargo metadata --no-deps` output trimmed to the fields zc reads.
fn metadata(json: &str) -> Metadata {
    serde_json::from_str(json).expect("fixture metadata parses")
}

#[test]
fn members_without_library_targets_are_skipped_with_a_reason() {
    let crates = classify_api_crates(metadata(
        r#"{
            "workspace_members": ["lib-id", "bin-id", "private-id", "macro-id", "both-id"],
            "packages": [
                {"id": "lib-id", "name": "zebra-lib", "targets": [{"kind": ["lib"]}, {"kind": ["bin"]}]},
                {"id": "bin-id", "name": "zebra-fuzz-targets", "targets": [{"kind": ["bin"]}]},
                {"id": "private-id", "name": "internal", "publish": [], "targets": [{"kind": ["lib"]}]},
                {"id": "macro-id", "name": "derive", "targets": [{"kind": ["proc-macro"]}]},
                {"id": "both-id", "name": "tool", "publish": [], "targets": [{"kind": ["bin"]}]},
                {"id": "dep-id", "name": "not-a-member", "targets": [{"kind": ["bin"]}]}
            ]
        }"#,
    ));

    assert_eq!(
        crates,
        ApiCrates {
            analysed: vec![
                "derive".to_string(),
                "internal".to_string(),
                "zebra-lib".to_string(),
            ],
            skipped: vec![
                SkippedCrate {
                    name: "tool".to_string(),
                    reason: SkipReason::NoLibrary,
                },
                SkippedCrate {
                    name: "zebra-fuzz-targets".to_string(),
                    reason: SkipReason::NoLibrary,
                },
            ],
        }
    );
}

#[test]
fn registry_restricted_libraries_are_analysed() {
    let crates = classify_api_crates(metadata(
        r#"{
            "workspace_members": ["id"],
            "packages": [
                {"id": "id", "name": "restricted", "publish": ["my-registry"], "targets": [{"kind": ["lib"]}]}
            ]
        }"#,
    ));

    assert_eq!(crates.analysed, vec!["restricted".to_string()]);
    assert!(crates.skipped.is_empty());
}
