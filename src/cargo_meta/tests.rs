use super::*;
use serde_json::{json, Value};

fn package(name: &str, dependencies: Vec<Value>) -> Value {
    json!({
        "id": name,
        "name": name,
        "rust_version": "1.80",
        "dependencies": dependencies,
        "features": {},
        "targets": [{"name": name, "kind": ["lib"]}]
    })
}

fn metadata(packages: Vec<Value>) -> Metadata {
    let members: Vec<_> = packages
        .iter()
        .map(|package| package["id"].clone())
        .collect();
    serde_json::from_value(json!({
        "packages": packages,
        "workspace_members": members
    }))
    .unwrap()
}

fn dependency(req: &str, kind: Option<&str>, target: Option<&str>) -> Value {
    json!({
        "name": "external",
        "rename": "alias",
        "req": req,
        "kind": kind,
        "target": target,
        "optional": false,
        "uses_default_features": true,
        "features": ["std"]
    })
}

#[test]
fn inventories_actual_library_targets_and_all_workspace_members() {
    let mut custom = package("custom-package", vec![]);
    custom["targets"] = json!([
        {"name": "custom-bin", "kind": ["bin"]},
        {"name": "actual_library", "kind": ["rlib", "cdylib"]}
    ]);
    custom["publish"] = json!(false);
    let mut proc_macro = package("macro-package", vec![]);
    proc_macro["targets"] = json!([{"name": "actual_macro", "kind": ["proc-macro"]}]);
    let mut binary = package("binary-package", vec![]);
    binary["targets"] = json!([{"name": "executable", "kind": ["bin"]}]);
    let inventory = classify_workspace_crates(metadata(vec![custom, proc_macro, binary]));
    assert_eq!(inventory.len(), 3);
    assert_eq!(
        inventory["custom-package"].lib_name.as_deref(),
        Some("actual_library")
    );
    assert_eq!(
        inventory["macro-package"].lib_name.as_deref(),
        Some("actual_macro")
    );
    assert_eq!(inventory["binary-package"].lib_name, None);
}

#[test]
fn feature_metadata_preserves_optional_renamed_dependencies() {
    let mut optional = dependency("^1", None, None);
    optional["optional"] = json!(true);
    let required = json!({"name": "required", "optional": false});
    let mut member = package("member", vec![optional, required]);
    member["features"] = json!({"tools": ["alias/std", "required/std"]});
    let tables = classify_feature_tables(metadata(vec![member]));
    assert_eq!(
        tables["member"].optional_dependencies,
        BTreeSet::from(["alias".into()])
    );
    assert_eq!(
        tables["member"].features["tools"],
        ["alias/std", "required/std"]
    );
}

#[test]
fn preserves_each_target_and_kind_row_including_changed_target_requirement() {
    let member = package(
        "member",
        vec![
            dependency("^1", None, None),
            dependency("^2", None, Some("cfg(windows)")),
            dependency("^3", Some("build"), Some("cfg(windows)")),
            dependency("^4", Some("dev"), None),
        ],
    );
    let base = classify_per_crate_deps(metadata(vec![member.clone()]));
    let mut changed = member;
    changed["dependencies"][1]["req"] = json!("^2.1");
    changed["dependencies"][2]["optional"] = json!(true);
    let head = classify_per_crate_deps(metadata(vec![changed]));
    assert_eq!(base.len(), 5);
    assert_eq!(head.len(), 5);
    let runtime_windows = |rows: &[PerCrateDep]| {
        rows.iter()
            .find(|row| {
                row.target.as_deref() == Some("cfg(windows)") && row.kind == DepKind::Runtime
            })
            .unwrap()
            .req
            .clone()
    };
    assert_eq!(runtime_windows(&base), "2");
    assert_eq!(runtime_windows(&head), "2.1");
    assert!(head
        .iter()
        .any(|row| row.kind == DepKind::Build && row.req == "3"));
    assert!(head
        .iter()
        .any(|row| row.kind == DepKind::Dev && row.req == "4"));
    assert!(head
        .iter()
        .any(|row| row.kind == DepKind::Build && row.optional));
    let msrv = head.iter().find(|row| row.scope == Scope::Msrv).unwrap();
    assert_eq!(msrv.target, None);
    assert_eq!(msrv.kind, DepKind::Unused);
    let encoded = encode_per_crate_cache(&head).unwrap();
    let decoded = decode_per_crate_cache(&encoded).unwrap();
    assert_eq!(decoded.len(), head.len());
    for (before, after) in head.iter().zip(&decoded) {
        assert_eq!(before.crate_name, after.crate_name);
        assert_eq!(before.dep, after.dep);
        assert_eq!(before.req, after.req);
        assert_eq!(before.scope, after.scope);
        assert_eq!(before.pkg, after.pkg);
        assert_eq!(before.target, after.target);
        assert_eq!(before.kind, after.kind);
        assert_eq!(before.optional, after.optional);
    }
}

#[test]
fn aggregation_retains_all_distinct_requirements_across_members_and_targets() {
    let first = package(
        "first",
        vec![
            dependency("^1", None, None),
            dependency("^2", None, Some("cfg(unix)")),
        ],
    );
    let second = package("second", vec![dependency("^2", None, None)]);
    let records = classify_workspace_deps(metadata(vec![first, second]));
    let record = &records["alias (pkg: external)"];
    assert_eq!(record.ver, "1 | 2");
    let encoded = encode_workspace_cache(&records).unwrap();
    assert_eq!(decode_workspace_cache(&encoded).unwrap(), records);
}

#[test]
fn caches_reject_old_schema_even_when_empty() {
    assert!(decode_per_crate_cache("[]").is_none());
    assert!(decode_workspace_cache("[]").is_none());
    assert!(decode_per_crate_cache(r#"{"schema":1,"rows":[]}"#).is_none());
    assert!(decode_workspace_cache(r#"{"schema":1,"rows":[]}"#).is_none());
    assert!(decode_per_crate_cache(r#"{"schema":2,"rows":[]}"#).is_none());
    assert!(decode_workspace_cache(r#"{"schema":2,"rows":[]}"#).is_none());
}

#[test]
fn workspace_diff_uses_real_member_rows_instead_of_joined_requirement_text() {
    let first = package("first", vec![dependency("^1", None, None)]);
    let second = package("second", vec![dependency("^2", None, None)]);
    let base_input = vec![first, second];
    let mut head_input = base_input.clone();
    head_input[0]["dependencies"][0]["req"] = json!("^3.0");
    let base = classify_workspace_deps(metadata(base_input.clone()));
    let head = classify_workspace_deps(metadata(head_input.clone()));
    let base_rows = classify_per_crate_deps(metadata(base_input));
    let head_rows = classify_per_crate_deps(metadata(head_input));

    let diff = crate::deps::diff(&base, &head, &base_rows, &head_rows);
    assert_eq!(diff.changed[0].name, "alias (pkg: external)");
    assert_eq!(diff.changed[0].old, "1 | 2");
    assert_eq!(diff.changed[0].new, "2 | 3.0");
    assert_eq!(diff.changed[0].bump, crate::model::Bump::Major);
    assert_eq!(diff.breaking, 1);
}

#[test]
fn workspace_diff_keeps_optional_and_build_migrations_separate_from_required_runtime() {
    for (kind, optional) in [(None, true), (Some("build"), false)] {
        let mut migrating = dependency("^1", kind, Some("cfg(windows)"));
        migrating["optional"] = json!(optional);
        let member = package("member", vec![dependency("^2", None, None), migrating]);
        let base_input = vec![member];
        let mut head_input = base_input.clone();
        head_input[0]["dependencies"][1]["req"] = json!("^3");
        let base = classify_workspace_deps(metadata(base_input.clone()));
        let head = classify_workspace_deps(metadata(head_input.clone()));
        let base_rows = classify_per_crate_deps(metadata(base_input));
        let head_rows = classify_per_crate_deps(metadata(head_input));

        let diff = crate::deps::diff(&base, &head, &base_rows, &head_rows);
        assert_eq!(diff.changed[0].kind, "runtime");
        assert_eq!(diff.changed[0].bump, crate::model::Bump::Major);
        assert_eq!(diff.breaking, 0);
    }
}
