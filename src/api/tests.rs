use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use serde_json::json;

use super::{
    configured_doc_dir, freeze_json, library_identity, one_sided_diff, parse_diff,
    valid_cached_json,
};
use crate::cache::{Cache, RunTmp};
use crate::model::CrateChange;

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn parses_all_diff_sections_and_interleaves_changed_pairs() {
    let output = "\
-outside section
+outside section

Removed items from the public API
=============================
-old::gone()
(none)

Changed items in the public API
=============================
-old::first()
+new::first()
= unchanged::item()
-old::second()

Added items to the public API
=============================
+new::added()
(none)

+new::also_added()
";

    let parsed = parse_diff(output);

    assert_eq!(parsed.removed, strings(&["old::gone()"]));
    assert_eq!(
        parsed.changed,
        strings(&[
            "  - old::first()",
            "  + new::first()",
            "  - old::second()",
            "  + ",
        ])
    );
    assert_eq!(
        parsed.added,
        strings(&["new::added()", "new::also_added()"])
    );
    assert_eq!(parsed.removed.len(), 1);
    assert_eq!(parsed.changed_old_count, 2);
    assert_eq!(parsed.added.len(), 2);
}

#[test]
fn frozen_artifacts_survive_target_collisions_and_failed_cache_writes() {
    let tmp = RunTmp::new().unwrap();
    let source = tmp.dir.join("same_lib_name.json");
    let cache = Cache {
        dir: tmp.dir.join("unavailable-cache"),
        script_hash: String::new(),
        api_fp: String::new(),
        api_json: Mutex::new(BTreeMap::new()),
    };

    fs::write(&source, br#"{"package":"foo-bar","api":"removed"}"#).unwrap();
    let first = freeze_json(&tmp, &source).unwrap();
    cache.copy_atomic("foo-bar.api.json", &first);
    fs::write(&source, br#"{"package":"foo_bar","api":"unchanged"}"#).unwrap();
    let second = freeze_json(&tmp, &source).unwrap();
    cache.copy_atomic("foo_bar.api.json", &second);

    assert_ne!(first, second);
    assert_eq!(
        fs::read_to_string(first).unwrap(),
        r#"{"package":"foo-bar","api":"removed"}"#
    );
    assert_eq!(
        fs::read_to_string(second).unwrap(),
        r#"{"package":"foo_bar","api":"unchanged"}"#
    );
    assert!(!cache.path("foo-bar.api.json").exists());
    assert!(!cache.path("foo_bar.api.json").exists());
}

#[test]
fn added_and_removed_libraries_are_compared_against_an_empty_api() {
    let output = "pub mod custom_lib\npub fn custom_lib::api()\n";
    let added = one_sided_diff(output, CrateChange::Added);
    assert!(added.removed.is_empty());
    assert!(added.changed.is_empty());
    assert_eq!(added.changed_old_count, 0);
    assert_eq!(
        added.added,
        strings(&["pub mod custom_lib", "pub fn custom_lib::api()"])
    );
    let removed = one_sided_diff(output, CrateChange::Removed);
    assert_eq!(removed.removed, added.added);
    assert!(removed.added.is_empty());
    assert!(removed.changed.is_empty());
    assert_eq!(removed.changed_old_count, 0);
}

#[test]
fn target_output_paths_use_cargo_target_names() {
    let target_dir = Path::new("/tmp/target");
    for (target, expected) in [
        (json!(null), "/tmp/target/doc"),
        (
            json!("x86_64-unknown-linux-gnu"),
            "/tmp/target/x86_64-unknown-linux-gnu/doc",
        ),
        (
            json!(["wasm32-unknown-unknown"]),
            "/tmp/target/wasm32-unknown-unknown/doc",
        ),
        (
            json!("thumbv8m.main-none-eabi"),
            "/tmp/target/thumbv8m.main-none-eabi/doc",
        ),
        (json!("/config/custom.v2.json"), "/tmp/target/custom.v2/doc"),
        (
            json!("/config/custom-platform.json"),
            "/tmp/target/custom-platform/doc",
        ),
    ] {
        assert_eq!(
            configured_doc_dir(target_dir, &target).unwrap(),
            Path::new(expected)
        );
    }
    assert!(configured_doc_dir(target_dir, &json!(["first", "second"])).is_err());
}

#[test]
fn library_transitions_keep_actual_library_identity() {
    assert_eq!(
        library_identity("package-name", None, Some("custom_lib")),
        (CrateChange::Added, "custom_lib".to_owned())
    );
    assert_eq!(
        library_identity("package-name", Some("custom_lib"), None),
        (CrateChange::Removed, "custom_lib".to_owned())
    );
    assert_eq!(
        library_identity("package-name", Some("old_lib"), Some("new_lib")),
        (CrateChange::Existing, "new_lib".to_owned())
    );
}

#[test]
fn invalid_rustdoc_cache_entries_are_not_reused() {
    let tmp = RunTmp::new().unwrap();
    let path = tmp.dir.join("cached.api.json");
    for contents in [
        "",
        "{",
        r#"{"format_version":57,"root":0,"index":{}}"#,
        r#"{"format_version":null,"root":null,"index":null}"#,
    ] {
        fs::write(&path, contents).unwrap();
        assert!(!valid_cached_json(&path));
    }
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "format_version": 57, "root": 0,
            "index": {"0": {"inner": {"module": {"items": []}}}},
            "paths": {}, "external_crates": {}
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(valid_cached_json(&path));
}
