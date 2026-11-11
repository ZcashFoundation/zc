use super::*;
use serde_json::json;

fn row(dep: &str, req: &str, kind: DepKind, target: Option<&str>) -> PerCrateDep {
    PerCrateDep {
        crate_name: "demo".into(),
        dep: dep.into(),
        req: req.into(),
        scope: Scope::External,
        pkg: "real-package".into(),
        kind,
        target: target.map(str::to_owned),
        optional: false,
    }
}

fn rustdoc(items: Vec<Value>) -> Value {
    let ids: Vec<_> = items.iter().map(|item| item["id"].clone()).collect();
    let mut index = serde_json::Map::new();
    index.insert(
        "0".into(),
        json!({
            "id": 0, "crate_id": 0, "name": "demo", "visibility": "public",
            "inner": {"module": {"items": ids}}
        }),
    );
    for item in items {
        index.insert(crate::rustdoc::id_text(&item["id"]).unwrap(), item);
    }
    json!({
        "root": 0, "index": index, "paths": {},
        "external_crates": {"1": {"name": "custom_library"}}
    })
}

fn function(type_id: u64) -> Value {
    json!({
        "id": 1, "crate_id": 0, "name": "make", "visibility": "public",
        "inner": {"function": {"sig": {
            "inputs": [], "output": {"resolved_path": {"path": "some::Type", "id": type_id}}
        }}}
    })
}

fn metadata(alias: &str, kind: Value, target: Value) -> Value {
    json!({
        "workspace_members": ["demo-id"],
        "packages": [
            {"id": "demo-id", "name": "demo"},
            {"id": "dep-id", "name": "real-package", "targets": [
                {"kind": ["lib"], "name": "custom_library"}
            ]}
        ],
        "resolve": {"nodes": [
            {"id": "demo-id", "deps": [
                {"name": alias, "pkg": "dep-id", "dep_kinds": [
                    {"kind": kind, "target": target}
                ]}
            ]},
            {"id": "dep-id", "deps": []}
        ]}
    })
}

#[test]
fn candidates_retain_runtime_target_requirements_without_build_contamination() {
    let base = vec![
        row("renamed", "1", DepKind::Runtime, None),
        row("renamed", "0.1", DepKind::Runtime, Some("cfg(windows)")),
        row("renamed", "5", DepKind::Build, None),
    ];
    let head = vec![
        row("renamed", "2", DepKind::Runtime, None),
        row("renamed", "0.2", DepKind::Runtime, Some("cfg(windows)")),
        row("renamed", "6", DepKind::Build, None),
    ];
    let tables = PubdepTables::build(&base, &head);
    let candidates = tables.candidates("demo");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].0.req, "1");
    assert_eq!(candidates[1].0.req, "0.1");
    assert_eq!(candidates[1].1.target.as_deref(), Some("cfg(windows)"));

    let build_only = PubdepTables::build(
        &[row("renamed", "1", DepKind::Build, None)],
        &[row("renamed", "2", DepKind::Build, None)],
    );
    assert!(build_only.candidates("demo").is_empty());
}

#[test]
fn local_modules_and_documentation_are_not_foreign_provenance() {
    let mut root = rustdoc(vec![
        function(2),
        json!({
            "id": 2, "crate_id": 0, "name": "Type", "visibility": "public",
            "docs": "real_package::Type and custom_library::Type",
            "links": {"foreign": 50},
            "inner": {"struct": {"kind": "unit", "impls": []}}
        }),
    ]);
    root["paths"]["2"] = json!({
        "crate_id": 0, "path": ["demo", "real_package", "Type"], "kind": "struct"
    });
    root["paths"]["50"] = json!({
        "crate_id": 1, "path": ["custom_library", "Type"], "kind": "struct"
    });
    assert!(foreign_crates(&root).unwrap().is_empty());
}

#[test]
fn private_field_types_do_not_make_dependencies_public() {
    let mut root = rustdoc(vec![json!({
        "id": 1, "crate_id": 0, "name": "Wrapper", "visibility": "public",
        "inner": {"struct": {
            "kind": {"plain": {"fields": [2], "has_stripped_fields": false}}, "impls": []
        }}
    })]);
    root["index"]["2"] = json!({
        "id": 2, "crate_id": 0, "name": "hidden", "visibility": "default",
        "inner": {"struct_field": {"resolved_path": {"path": "custom_library::Type", "id": 50}}}
    });
    root["paths"]["50"] = json!({"crate_id": 1, "path": ["custom_library", "Type"]});
    assert!(foreign_crates(&root).unwrap().is_empty());
}

#[test]
fn public_signatures_and_reexports_keep_foreign_crate_provenance() {
    for item in [
        function(50),
        json!({
            "id": 1, "crate_id": 0, "name": null, "visibility": "public",
            "inner": {"use": {
                "id": 50, "name": "Exported", "source": "custom_library::Type", "is_glob": false
            }}
        }),
    ] {
        let mut root = rustdoc(vec![item]);
        root["paths"]["50"] = json!({
            "crate_id": 1, "path": ["custom_library", "Type"], "kind": "struct"
        });
        assert_eq!(
            foreign_crates(&root).unwrap(),
            HashSet::from(["custom_library".into()])
        );
    }
}

#[test]
fn foreign_glob_reexports_keep_their_module_provenance() {
    let mut root = rustdoc(vec![json!({
        "id": 1, "crate_id": 0, "name": null, "visibility": "public",
        "inner": {"use": {
            "id": 50, "name": "custom_library", "source": "custom_library", "is_glob": true
        }}
    })]);
    root["paths"]["50"] = json!({
        "crate_id": 1, "path": ["custom_library"], "kind": "module"
    });
    assert_eq!(
        foreign_crates(&root).unwrap(),
        HashSet::from(["custom_library".into()])
    );
}

#[test]
fn foreign_trait_impl_headers_are_public_provenance_without_methods() {
    let mut root = rustdoc(vec![json!({
        "id": 1, "crate_id": 0, "name": "Local", "visibility": "public",
        "inner": {"struct": {"kind": "unit", "impls": [2]}}
    })]);
    root["index"]["2"] = json!({
        "id": 2, "crate_id": 0, "name": null, "visibility": "default",
        "inner": {"impl": {
            "trait": {"path": "custom_library::Trait", "id": 50},
            "for": {"resolved_path": {"path": "demo::Local", "id": 1}},
            "items": [], "is_negative": false, "generics": {"params": [], "where_predicates": []}
        }}
    });
    root["paths"]["1"] = json!({"crate_id": 0, "path": ["demo", "Local"], "kind": "struct"});
    root["paths"]["50"] = json!({
        "crate_id": 1, "path": ["custom_library", "Trait"], "kind": "trait"
    });
    assert_eq!(
        foreign_crates(&root).unwrap(),
        HashSet::from(["custom_library".into()])
    );
}

#[test]
fn public_traits_expose_foreign_implementation_owners_even_without_methods() {
    for methods in [vec![], vec![3]] {
        let mut root = rustdoc(vec![json!({
            "id": 1, "crate_id": 0, "name": "Marker", "visibility": "public",
            "inner": {"trait": {"items": [], "implementations": [2]}}
        })]);
        root["index"]["2"] = json!({
            "id": 2, "crate_id": 0, "name": null, "visibility": "default",
            "inner": {"impl": {
                "trait": {"path": "demo::Marker", "id": 1},
                "for": {"resolved_path": {"path": "custom_library::S", "id": 50}},
                "items": methods, "is_negative": false,
                "generics": {"params": [], "where_predicates": []}
            }}
        });
        root["index"]["3"] = json!({
            "id": 3, "crate_id": 0, "name": "method", "visibility": "default",
            "inner": {"function": {"sig": {"inputs": [], "output": null}}}
        });
        root["paths"]["50"] = json!({
            "crate_id": 1, "path": ["custom_library", "S"], "kind": "struct"
        });
        let foreign = foreign_crates(&root).unwrap();
        assert_eq!(foreign, HashSet::from(["custom_library".into()]));
        let mut old = row("bar", "0.1", DepKind::Runtime, None);
        old.optional = true;
        let mut new = row("bar", "0.2", DepKind::Runtime, None);
        new.optional = true;
        let tables = PubdepTables::build(&[old], &[new]);
        let metadata = metadata("bar", Value::Null, Value::Null);
        let result = findings(
            &tables.candidates("demo"),
            Some((&foreign, &metadata)),
            (&foreign, &metadata),
            "demo",
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].is_breaking());
    }
}

#[test]
fn foreign_owner_associated_types_keep_dependency_provenance() {
    let mut root = rustdoc(vec![json!({
        "id": 1, "crate_id": 0, "name": "Project", "visibility": "public",
        "inner": {"trait": {"items": [], "implementations": [2]}}
    })]);
    root["index"]["2"] = json!({
        "id": 2, "crate_id": 0, "name": null, "visibility": "default",
        "inner": {"impl": {
            "trait": {"path": "demo::Project", "id": 1},
            "for": {"resolved_path": {"path": "owner_library::Owner", "id": 60}},
            "items": [3], "is_negative": false,
            "generics": {"params": [], "where_predicates": []}
        }}
    });
    root["index"]["3"] = json!({
        "id": 3, "crate_id": 0, "name": "Output", "visibility": "default",
        "inner": {"assoc_type": {
            "generics": {"params": [], "where_predicates": []}, "bounds": [],
            "type": {"resolved_path": {"path": "custom_library::Token", "id": 50}}
        }}
    });
    root["paths"]["50"] = json!({
        "crate_id": 1, "path": ["custom_library", "Token"], "kind": "struct"
    });
    root["paths"]["60"] = json!({
        "crate_id": 2, "path": ["owner_library", "Owner"], "kind": "struct"
    });
    root["external_crates"]["2"] = json!({"name": "owner_library"});
    let foreign = foreign_crates(&root).unwrap();
    assert_eq!(
        foreign,
        HashSet::from(["custom_library".into(), "owner_library".into()])
    );
    let tables = PubdepTables::build(
        &[row("renamed", "0.1", DepKind::Runtime, None)],
        &[row("renamed", "0.2", DepKind::Runtime, None)],
    );
    let metadata = metadata("renamed", Value::Null, Value::Null);
    let result = findings(
        &tables.candidates("demo"),
        Some((&foreign, &metadata)),
        (&foreign, &metadata),
        "demo",
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].is_breaking());
}

#[test]
fn private_owners_and_unrelated_private_impls_do_not_expose_foreign_provenance() {
    let mut root = rustdoc(vec![json!({
        "id": 1, "crate_id": 0, "name": "Marker", "visibility": "public",
        "docs": "custom_library::S", "links": {"foreign": 50},
        "inner": {"trait": {"items": [], "implementations": [2]}}
    })]);
    root["index"]["2"] = json!({
        "id": 2, "crate_id": 0, "name": null, "visibility": "default",
        "inner": {"impl": {
            "trait": {"path": "demo::Marker", "id": 1},
            "for": {"resolved_path": {"path": "demo::Hidden", "id": 3}},
            "items": [], "is_negative": false,
            "generics": {"where_predicates": [{
                "bound_predicate": {"type": {"resolved_path": {"path": "custom_library::S", "id": 50}}}
            }]}
        }}
    });
    root["index"]["3"] = json!({
        "id": 3, "crate_id": 0, "name": "Hidden", "visibility": "default",
        "inner": {"struct": {"kind": "unit", "impls": [2]}}
    });
    root["index"]["4"] = json!({
        "id": 4, "crate_id": 0, "name": "Unrelated", "visibility": "default",
        "inner": {"trait": {"items": [], "implementations": [5]}}
    });
    root["index"]["5"] = json!({
        "id": 5, "crate_id": 0, "name": null, "visibility": "default",
        "inner": {"impl": {
            "trait": {"path": "demo::Unrelated", "id": 4},
            "for": {"resolved_path": {"path": "custom_library::S", "id": 50}},
            "items": [], "is_negative": false
        }}
    });
    root["paths"]["50"] = json!({
        "crate_id": 1, "path": ["custom_library", "S"], "kind": "struct"
    });
    assert!(foreign_crates(&root).unwrap().is_empty());
}

#[test]
fn optional_alias_package_replacements_break_identity_even_with_equal_requirements() {
    for new_req in ["0.1", "0.2"] {
        let mut old = row("foreign_api", "0.1", DepKind::Runtime, None);
        old.pkg = "old-pkg".into();
        old.optional = true;
        let mut new = row("foreign_api", new_req, DepKind::Runtime, None);
        new.pkg = "new-pkg".into();
        new.optional = true;
        let tables = PubdepTables::build(&[old], &[new]);
        let candidates = tables.candidates("demo");
        assert_eq!(candidates.len(), 1);
        let mut head = rustdoc(vec![function(50)]);
        head["paths"]["50"] = json!({
            "crate_id": 1, "path": ["foreign_api", "Token"], "kind": "struct"
        });
        head["external_crates"]["1"]["name"] = json!("foreign_api");
        let mut metadata = metadata("foreign_api", Value::Null, Value::Null);
        metadata["packages"][1]["name"] = json!("new-pkg");
        metadata["packages"][1]["targets"][0]["name"] = json!("foreign_api");
        let mut base_metadata = metadata.clone();
        base_metadata["packages"][1]["name"] = json!("old-pkg");
        let foreign = foreign_crates(&head).unwrap();
        let result = findings(
            &candidates,
            Some((&foreign, &base_metadata)),
            (&foreign, &metadata),
            "demo",
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let finding = &result[0];
        assert_eq!(finding.dep, "foreign_api");
        assert_eq!(finding.old_package, "old-pkg");
        assert_eq!(finding.new_package, "new-pkg");
        assert_eq!(finding.old, "0.1");
        assert_eq!(finding.new, new_req);
        assert!(finding.is_breaking());
    }
}

#[test]
fn identity_changes_require_actual_baseline_package_exposure() {
    for replacement in [false, true] {
        let mut old = row("renamed", "0.1", DepKind::Runtime, None);
        old.optional = true;
        let mut new = row(
            "renamed",
            if replacement { "0.1" } else { "0.2" },
            DepKind::Runtime,
            None,
        );
        new.optional = true;
        if replacement {
            new.pkg = "replacement-package".into();
        }
        let tables = PubdepTables::build(&[old], &[new]);
        let candidates = tables.candidates("demo");
        let mut base_metadata = metadata("renamed", Value::Null, Value::Null);
        base_metadata["packages"][1]["targets"][0]["name"] = json!("old_library");
        let mut head_metadata = metadata("renamed", Value::Null, Value::Null);
        if replacement {
            head_metadata["packages"][1]["name"] = json!("replacement-package");
        }
        let mut head = rustdoc(vec![function(50)]);
        head["paths"]["50"] = json!({"crate_id": 1});
        let head_foreign = foreign_crates(&head).unwrap();
        for was_public in [false, true] {
            let mut base = rustdoc(if was_public {
                vec![function(50)]
            } else {
                vec![]
            });
            base["paths"]["50"] = json!({"crate_id": 1});
            base["external_crates"]["1"]["name"] = json!("old_library");
            let base_foreign = foreign_crates(&base).unwrap();
            let result = findings(
                &candidates,
                Some((&base_foreign, &base_metadata)),
                (&head_foreign, &head_metadata),
                "demo",
            )
            .unwrap();
            assert_eq!(result.len(), usize::from(was_public));
            if let Some(finding) = result.first() {
                assert!(finding.is_breaking());
                assert_eq!(finding.old_package, "real-package");
                assert_eq!(
                    finding.new_package,
                    if replacement {
                        "replacement-package"
                    } else {
                        "real-package"
                    }
                );
            }
        }
        // No baseline library means an entirely additive library surface.
        assert!(
            findings(&candidates, None, (&head_foreign, &json!({})), "demo",)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn another_baseline_package_with_the_same_library_name_is_not_old_exposure() {
    let old = row("renamed", "0.1", DepKind::Runtime, None);
    let new = row("renamed", "0.2", DepKind::Runtime, None);
    let foreign = HashSet::from(["custom_library".into()]);
    let head = metadata("renamed", Value::Null, Value::Null);
    let mut base = head.clone();
    base["packages"][1]["name"] = json!("unrelated-package");
    assert!(findings(
        &[(&old, &new)],
        Some((&foreign, &base)),
        (&foreign, &head),
        "demo",
    )
    .unwrap()
    .is_empty());
}

#[test]
fn requirement_only_changes_keep_existing_breaking_and_review_classification() {
    for (old_req, new_req, breaking) in [
        ("0.1", "0.2", Some(true)),
        (">=1", ">=2", Some(false)),
        ("0.1", "0.1", None),
    ] {
        let tables = PubdepTables::build(
            &[row("renamed", old_req, DepKind::Runtime, None)],
            &[row("renamed", new_req, DepKind::Runtime, None)],
        );
        let foreign = HashSet::from(["custom_library".into()]);
        let metadata = metadata("renamed", Value::Null, Value::Null);
        let result = findings(
            &tables.candidates("demo"),
            Some((&foreign, &metadata)),
            (&foreign, &metadata),
            "demo",
        )
        .unwrap();
        assert_eq!(result.first().map(PubdepFinding::is_breaking), breaking);
        if let Some(finding) = result.first() {
            assert_eq!(finding.old_package, "real-package");
            assert_eq!(finding.new_package, "real-package");
            assert_eq!(finding.old, old_req);
            assert_eq!(finding.new, new_req);
        }
    }
}

#[test]
fn alias_renames_keep_major_review_and_unchanged_requirements() {
    for (old_req, new_req, breaking) in [
        ("0.1", "0.2", Some(true)),
        (">=1", ">=2", Some(false)),
        ("0.1", "0.1", None),
    ] {
        let mut old = row("old", old_req, DepKind::Runtime, None);
        old.optional = true;
        let mut new = row("new", new_req, DepKind::Runtime, None);
        new.optional = true;
        let tables = PubdepTables::build(&[old], &[new]);
        let foreign = HashSet::from(["custom_library".into()]);
        let base = metadata("old", Value::Null, Value::Null);
        let head = metadata("new", Value::Null, Value::Null);
        let result = findings(
            &tables.candidates("demo"),
            Some((&foreign, &base)),
            (&foreign, &head),
            "demo",
        )
        .unwrap();
        assert_eq!(result.first().map(PubdepFinding::is_breaking), breaking);
        if let Some(finding) = result.first() {
            assert_eq!(finding.dep, "new");
            assert_eq!(finding.old_package, "real-package");
            assert_eq!(finding.new_package, "real-package");
            assert_eq!(finding.old, old_req);
            assert_eq!(finding.new, new_req);
        }
    }
}

#[test]
fn alias_renames_pair_runtime_rows_by_selected_target() {
    let tables = PubdepTables::build(
        &[
            row("old", "0.1", DepKind::Runtime, None),
            row("old", "1", DepKind::Runtime, Some("cfg(windows)")),
            row("old", "5", DepKind::Build, Some("cfg(windows)")),
        ],
        &[
            row("new", "0.2", DepKind::Runtime, None),
            row("new", "2", DepKind::Runtime, Some("cfg(windows)")),
            row("new", "6", DepKind::Build, Some("cfg(windows)")),
        ],
    );
    let candidates = tables.candidates("demo");
    assert_eq!(candidates.len(), 2);
    let foreign = HashSet::from(["custom_library".into()]);
    let base = metadata("old", Value::Null, json!("cfg(windows)"));
    let head = metadata("new", Value::Null, json!("cfg(windows)"));
    let result = findings(
        &candidates,
        Some((&foreign, &base)),
        (&foreign, &head),
        "demo",
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].old, "1");
    assert_eq!(result[0].new, "2");
    assert_eq!(result[0].target.as_deref(), Some("cfg(windows)"));
    for head in [
        metadata("new", json!("build"), json!("cfg(windows)")),
        metadata("new", Value::Null, json!("cfg(unix)")),
    ] {
        assert!(findings(
            &candidates,
            Some((&foreign, &base)),
            (&foreign, &head),
            "demo",
        )
        .unwrap()
        .is_empty());
    }
}

#[test]
fn exact_alias_replacements_take_precedence_over_package_rename_matches() {
    let mut replaced = row("stable", "0.1", DepKind::Runtime, None);
    replaced.pkg = "replaced-package".into();
    let tables = PubdepTables::build(
        &[row("old", "0.1", DepKind::Runtime, None), replaced],
        &[
            row("stable", "0.2", DepKind::Runtime, None),
            row("new", "0.2", DepKind::Runtime, None),
        ],
    );
    let candidates = tables.candidates("demo");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].0.dep, "stable");
    assert_eq!(candidates[0].0.pkg, "replaced-package");
    assert_eq!(candidates[0].1.dep, "stable");
    assert_eq!(candidates[1].0.dep, "old");
    assert_eq!(candidates[1].1.dep, "new");
}

#[test]
fn ambiguous_alias_correspondence_errors_only_for_exposed_changes() {
    for reverse in [false, true] {
        let multiple = vec![
            row("one", "0.1", DepKind::Runtime, None),
            // An unchanged alternative must not be silently discarded.
            row("two", "0.2", DepKind::Runtime, None),
        ];
        let single = vec![row("renamed", "0.2", DepKind::Runtime, None)];
        let (base_rows, head_rows) = if reverse {
            (&single, &multiple)
        } else {
            (&multiple, &single)
        };
        let tables = PubdepTables::build(base_rows, head_rows);
        let candidates = tables.candidates("demo");
        assert_eq!(candidates.len(), 2);
        let mut base = metadata(&base_rows[0].dep, Value::Null, Value::Null);
        let mut head = metadata(&head_rows[0].dep, Value::Null, Value::Null);
        let extra = json!({
            "name": "two", "pkg": "dep-id",
            "dep_kinds": [{"kind": null, "target": null}]
        });
        let multiple_metadata = if reverse { &mut head } else { &mut base };
        multiple_metadata["resolve"]["nodes"][0]["deps"]
            .as_array_mut()
            .unwrap()
            .push(extra);
        let public = HashSet::from(["custom_library".into()]);
        let private = HashSet::from(["unrelated_library".into()]);
        for (base_foreign, head_foreign) in [
            (&private, &private),
            (&private, &public),
            (&public, &private),
        ] {
            assert!(findings(
                &candidates,
                Some((base_foreign, &base)),
                (head_foreign, &head),
                "demo",
            )
            .unwrap()
            .is_empty());
        }
        let error = findings(
            &candidates,
            Some((&public, &base)),
            (&public, &head),
            "demo",
        )
        .unwrap_err();
        assert!(error.contains("ambiguous alias correspondence"));
    }
}

#[test]
fn private_alias_alternatives_do_not_make_public_correspondence_ambiguous() {
    for reverse in [false, true] {
        let multiple = vec![
            row("one", "0.1", DepKind::Runtime, None),
            row("two", "0.2", DepKind::Runtime, None),
        ];
        let single = vec![row("renamed", "0.3", DepKind::Runtime, None)];
        let (base_rows, head_rows) = if reverse {
            (&single, &multiple)
        } else {
            (&multiple, &single)
        };
        let tables = PubdepTables::build(base_rows, head_rows);
        let candidates = tables.candidates("demo");
        let mut base = metadata(&base_rows[0].dep, Value::Null, Value::Null);
        let mut head = metadata(&head_rows[0].dep, Value::Null, Value::Null);
        let multiple_metadata = if reverse { &mut head } else { &mut base };
        multiple_metadata["packages"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "private-id", "name": "real-package",
                "targets": [{"kind": ["lib"], "name": "private_library"}]
            }));
        multiple_metadata["resolve"]["nodes"][0]["deps"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "name": "two", "pkg": "private-id",
                "dep_kinds": [{"kind": null, "target": null}]
            }));
        multiple_metadata["resolve"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "private-id", "deps": []}));
        let public = HashSet::from(["custom_library".into()]);
        let result = findings(
            &candidates,
            Some((&public, &base)),
            (&public, &head),
            "demo",
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].dep, head_rows[0].dep);
        assert_eq!(result[0].old, base_rows[0].req);
        assert!(result[0].is_breaking());
    }
}

#[test]
fn custom_library_names_and_renamed_dependencies_join_to_real_packages() {
    for (display, resolved) in [("real-package", "custom_library"), ("renamed", "renamed")] {
        let old = row(display, "1", DepKind::Runtime, None);
        let new = row(display, "2", DepKind::Runtime, None);
        let foreign = HashSet::from(["custom_library".into()]);
        let metadata = metadata(resolved, Value::Null, Value::Null);
        let result = findings(
            &[(&old, &new)],
            Some((&foreign, &metadata)),
            (&foreign, &metadata),
            "demo",
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].dep, display);
    }
}

#[test]
fn findings_preserve_target_and_require_a_runtime_resolution() {
    let old = row("renamed", "1", DepKind::Runtime, Some("cfg(windows)"));
    let new = row("renamed", "2", DepKind::Runtime, Some("cfg(windows)"));
    let foreign = HashSet::from(["custom_library".into()]);
    let matching = metadata("renamed", Value::Null, json!("cfg(windows)"));
    let result = findings(
        &[(&old, &new)],
        Some((&foreign, &matching)),
        (&foreign, &matching),
        "demo",
    )
    .unwrap();
    assert_eq!(result[0].target.as_deref(), Some("cfg(windows)"));
    for metadata in [
        metadata("renamed", json!("build"), json!("cfg(windows)")),
        metadata("renamed", Value::Null, json!("cfg(unix)")),
    ] {
        assert!(findings(
            &[(&old, &new)],
            Some((&foreign, &matching)),
            (&foreign, &metadata),
            "demo"
        )
        .unwrap()
        .is_empty());
        assert!(findings(
            &[(&old, &new)],
            Some((&foreign, &metadata)),
            (&foreign, &matching),
            "demo"
        )
        .unwrap()
        .is_empty());
    }
}

#[test]
fn malformed_provenance_is_an_error_not_an_empty_finding_set() {
    assert!(foreign_crates(&json!({})).is_err());
    let old = row("renamed", "1", DepKind::Runtime, None);
    let new = row("renamed", "2", DepKind::Runtime, None);
    let foreign = HashSet::from(["custom_library".into()]);
    assert!(findings(
        &[(&old, &new)],
        Some((&foreign, &json!({}))),
        (&foreign, &json!({})),
        "demo"
    )
    .is_err());
    let mut root = rustdoc(vec![function(50)]);
    root["paths"]["50"] = json!({"path": ["custom_library", "Type"]});
    assert!(foreign_crates(&root).is_err());
    root["paths"]["50"] = json!({"crate_id": 9});
    assert!(foreign_crates(&root).is_err());
}

#[test]
fn ids_that_rustdoc_stripped_from_the_json_are_local_not_malformed() {
    let root = rustdoc(vec![function(50)]);
    assert!(foreign_crates(&root).unwrap().is_empty());
}

#[test]
fn bitflags_internal_type_does_not_hide_the_foreign_trait_provenance() {
    // `bitflags!` emits `impl PublicFlags for Flags { type Internal = InternalBitFlags; }`.
    // The struct is private, so rustdoc leaves it out of `index` and `paths`.
    let mut root = rustdoc(vec![json!({
        "id": 1, "crate_id": 0, "name": "Flags", "visibility": "public",
        "inner": {"struct": {"kind": "unit", "impls": [2]}}
    })]);
    root["index"]["2"] = json!({
        "id": 2, "crate_id": 0, "name": null, "visibility": "default",
        "inner": {"impl": {
            "trait": {"path": "PublicFlags", "id": 50},
            "for": {"resolved_path": {"path": "Flags", "id": 1}},
            "items": [3], "is_negative": false,
            "generics": {"params": [], "where_predicates": []}
        }}
    });
    root["index"]["3"] = json!({
        "id": 3, "crate_id": 0, "name": "Internal", "visibility": "default",
        "inner": {"assoc_type": {
            "generics": {"params": [], "where_predicates": []}, "bounds": [],
            "type": {"resolved_path": {"path": "InternalBitFlags", "id": 99}}
        }}
    });
    root["paths"]["1"] = json!({"crate_id": 0, "path": ["demo", "Flags"], "kind": "struct"});
    root["paths"]["50"] = json!({
        "crate_id": 1, "path": ["custom_library", "PublicFlags"], "kind": "trait"
    });
    assert_eq!(
        foreign_crates(&root).unwrap(),
        HashSet::from(["custom_library".into()])
    );
}

#[test]
fn ambiguous_library_names_are_not_invented_package_provenance() {
    let old = row("renamed", "1", DepKind::Runtime, None);
    let new = row("renamed", "2", DepKind::Runtime, None);
    let mut metadata = metadata("renamed", Value::Null, Value::Null);
    metadata["packages"].as_array_mut().unwrap().push(json!({
        "id": "other-package-id", "name": "other-package",
        "targets": [{"name": "custom_library", "kind": ["lib"]}]
    }));
    metadata["resolve"]["nodes"][0]["deps"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name": "other", "pkg": "other-package-id",
            "dep_kinds": [{"kind": "build", "target": null}]
        }));
    metadata["resolve"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "other-package-id", "deps": []
        }));
    // A build-only package with a colliding library name cannot contaminate the join.
    let foreign = HashSet::from(["custom_library".into()]);
    assert_eq!(
        findings(
            &[(&old, &new)],
            Some((&foreign, &metadata)),
            (&foreign, &metadata),
            "demo"
        )
        .unwrap()
        .len(),
        1
    );
    metadata["resolve"]["nodes"][1]["deps"] = json!([{
        "name": "other", "pkg": "other-package-id",
        "dep_kinds": [{"kind": null, "target": null}]
    }]);
    assert!(findings(
        &[(&old, &new)],
        Some((&foreign, &metadata)),
        (&foreign, &metadata),
        "demo"
    )
    .is_err());
}

#[test]
fn unrelated_unchanged_ambiguous_libraries_do_not_block_a_private_update() {
    let mut base_rows = Vec::new();
    for (alias, package, optional) in [
        ("left", "left-pkg", false),
        ("right", "right-pkg", false),
        ("hidden", "hidden", true),
    ] {
        let mut dependency = row(alias, "0.1", DepKind::Runtime, None);
        dependency.pkg = package.into();
        dependency.optional = optional;
        base_rows.push(dependency);
    }
    let mut head_rows = base_rows.clone();
    head_rows[2].req = "0.2".into();
    let tables = PubdepTables::build(&base_rows, &head_rows);
    let candidates = tables.candidates("demo");
    let mut metadata = metadata("left", Value::Null, Value::Null);
    metadata["packages"][1]["name"] = json!("left-pkg");
    metadata["packages"][1]["targets"][0]["name"] = json!("shared");
    for (alias, package, library) in [
        ("right", "right-pkg", "shared"),
        ("hidden", "hidden", "hidden_lib"),
    ] {
        metadata["packages"].as_array_mut().unwrap().push(json!({
            "id": alias, "name": package,
            "targets": [{"kind": ["lib"], "name": library}]
        }));
        metadata["resolve"]["nodes"][0]["deps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": alias, "pkg": alias,
                "dep_kinds": [{"kind": null, "target": null}]}));
        metadata["resolve"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": alias, "deps": []}));
    }
    for hidden_public in [false, true] {
        let mut foreign = HashSet::from(["shared".into()]);
        if hidden_public {
            foreign.insert("hidden_lib".into());
        }
        let result = findings(
            &candidates,
            Some((&foreign, &metadata)),
            (&foreign, &metadata),
            "demo",
        )
        .unwrap();
        assert_eq!(result.len(), usize::from(hidden_public));
        if let Some(finding) = result.first() {
            assert_eq!(finding.dep, "hidden");
            assert!(finding.is_breaking());
        }
    }
}
