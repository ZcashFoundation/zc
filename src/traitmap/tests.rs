use super::*;
use serde_json::json;

#[test]
fn inherent_and_multiple_trait_members_block_false_attribution() {
    let json = json!({"index": {
        "trait": {"inner": {"impl": {
            "trait": {"path": "core::default::Default"},
            "for": {"resolved_path": {"path": "fixture::S"}},
            "items": ["default"]
        }}},
        "inherent": {"inner": {"impl": {
            "trait": null,
            "for": {"resolved_path": {"path": "fixture::S"}},
            "items": ["inherent-default"]
        }}},
        "default": {"name": "default"},
        "inherent-default": {"name": "default"}
    }});
    let map = rows_to_map(&extract(&json.to_string()));
    assert_eq!(
        map.get(&("fixture::S".into(), "default".into())),
        Some(&None)
    );

    let rows = vec![
        (
            "fixture::S".into(),
            "from".into(),
            "core::convert::From<u8>".into(),
        ),
        (
            "fixture::S".into(),
            "from".into(),
            "core::convert::From<u16>".into(),
        ),
        (
            "fixture::S".into(),
            "from".into(),
            "core::convert::From<u8>".into(),
        ),
    ];
    let map = rows_to_map(&rows);
    assert_eq!(map.get(&("fixture::S".into(), "from".into())), Some(&None));
}

#[test]
fn resolved_paths_preserve_owner_modules_and_trait_generic_arguments() {
    let json = json!({
        "paths": {
            "left": {"path": ["fixture", "left", "S"]},
            "right": {"path": ["fixture", "right", "S"]},
            "trait": {"path": ["dependency", "Trait"]}
        },
        "index": {
            "left-impl": {"inner": {"impl": {
                "trait": {"path": "Trait", "id": "trait", "args": {
                    "angle_bracketed": {"args": [{"type": {"primitive": "u8"}}],
                        "constraints": []}
                }},
                "for": {"resolved_path": {"path": "S", "id": "left", "args": {
                    "angle_bracketed": {"args": [{"type": {"primitive": "u32"}}],
                        "constraints": []}
                }}},
                "items": ["method"]
            }}},
            "right-impl": {"inner": {"impl": {
                "trait": null,
                "for": {"resolved_path": {"path": "S", "id": "right"}},
                "items": ["method"]
            }}},
            "method": {"name": "item"}
        }
    });
    let map = rows_to_map(&extract(&json.to_string()));
    assert_eq!(
        map.get(&("fixture::left::S<u32>".into(), "item".into())),
        Some(&Some("dependency::Trait<u8>".into()))
    );
    assert_eq!(
        map.get(&("fixture::right::S".into(), "item".into())),
        Some(&None)
    );
}

#[test]
fn reexported_owners_keep_trait_attribution_under_the_public_alias() {
    let json = json!({
        "root": "root",
        "paths": {"owner": {"path": ["fixture", "private", "S"]}},
        "index": {
            "root": {"id": "root", "name": "fixture", "inner": {"module": {
                "items": ["export"]
            }}},
            "export": {"id": "export", "visibility": "public", "inner": {"use": {
                "id": "owner", "name": "Alias", "is_glob": false
            }}},
            "owner": {"id": "owner", "name": "S", "inner": {"struct": {
                "kind": "unit", "impls": ["implementation"]
            }}},
            "implementation": {"id": "implementation", "inner": {"impl": {
                "trait": {"path": "dependency::Trait"},
                "for": {"resolved_path": {"id": "owner", "path": "S"}},
                "items": ["method"]
            }}},
            "method": {"name": "item"}
        }
    });
    let map = rows_to_map(&extract(&json.to_string()));
    assert_eq!(
        map.get(&("fixture::Alias".into(), "item".into())),
        Some(&Some("dependency::Trait".into()))
    );
}

#[test]
fn reexported_traits_group_with_the_explicit_public_impl_header() {
    let json = json!({
        "root": "root",
        "paths": {
            "trait": {"path": ["fixture", "hidden", "T"]},
            "owner": {"path": ["fixture", "model", "S"]}
        },
        "index": {
            "root": {"id": "root", "name": "fixture", "inner": {"module": {
                "items": ["hidden", "export", "model"]
            }}},
            "hidden": {"id": "hidden", "name": "hidden", "visibility": "default",
                "inner": {"module": {"items": ["trait"]}}},
            "export": {"id": "export", "visibility": "public", "inner": {"use": {
                "id": "trait", "name": "T", "is_glob": false
            }}},
            "trait": {"id": "trait", "name": "T", "visibility": "public",
                "inner": {"trait": {"items": []}}},
            "model": {"id": "model", "name": "model", "visibility": "public",
                "inner": {"module": {"items": ["owner"]}}},
            "owner": {"id": "owner", "name": "S", "visibility": "public",
                "inner": {"struct": {"kind": "unit", "impls": ["implementation"]}}},
            "implementation": {"id": "implementation", "inner": {"impl": {
                "trait": {"id": "trait", "path": "T", "args": {"angle_bracketed": {
                    "args": [{"type": {"primitive": "u8"}}], "constraints": []
                }}},
                "for": {"resolved_path": {"id": "owner", "path": "S", "args": {
                    "angle_bracketed": {
                        "args": [{"type": {"primitive": "u32"}}], "constraints": []
                    }
                }}},
                "items": ["method"]
            }}},
            "method": {"id": "method", "name": "go", "inner": {"function": {}}}
        }
    });
    let map = rows_to_map(&extract(&json.to_string()));
    assert_eq!(
        map.get(&("fixture::model::S<u32>".into(), "go".into())),
        Some(&Some("fixture::T<u8>".into()))
    );
    let lines = vec![
        "impl fixture::T<u8> for fixture::model::S<u32>".to_string(),
        "pub fn fixture::model::S<u32>::go(&self)".to_string(),
    ];
    assert_eq!(
        crate::changelog::render(&lines, crate::model::Section::Added, "fixture", &map),
        vec!["- `impl T<u8> for model::S<u32>`:", "  - `go`"],
    );
}

#[test]
fn cached_ambiguity_cannot_become_a_trait_claim() {
    let rows = vec![
        ("fixture::S".into(), "default".into(), String::new()),
        (
            "fixture::S".into(),
            "default".into(),
            "core::default::Default".into(),
        ),
    ];
    let map = parse_tsv(&render_tsv(&rows));
    assert_eq!(
        map.get(&("fixture::S".into(), "default".into())),
        Some(&None)
    );
}

#[test]
fn nested_generic_identity_keeps_abi_lifetimes_constants_and_bindings() {
    let root = json!({"paths": {
        "trait": {"path": ["dependency", "Trait"]},
        "owner": {"path": ["fixture", "S"]}
    }});
    let path = json!({"id": "trait", "path": "Trait", "args": {"angle_bracketed": {
        "args": [
            {"lifetime": "'a"},
            {"const": {"expr": "16"}},
            {"type": {"function_pointer": {
                "generic_params": [],
                "header": {"is_unsafe": true, "abi": {"C": {"unwind": true}}},
                "sig": {
                    "inputs": [["value", {"borrowed_ref": {
                        "lifetime": "'a", "is_mutable": true, "type": {"primitive": "u8"}
                    }}]],
                    "output": {"array": {"type": {"primitive": "u8"}, "len": "16"}},
                    "is_c_variadic": false
                }
            }}}
        ],
        "constraints": [{
            "name": "Item", "args": null, "binding": {"equality": {"type": {
                "qualified_path": {
                    "self_type": {"resolved_path": {"id": "owner", "path": "S"}},
                    "trait": null, "name": "Value", "args": null
                }
            }}}
        }]
    }}});
    let identity = path_name(&root, &path).unwrap();
    for part in [
        "dependency::Trait<'a, 16",
        "unsafe extern \"C-unwind\" fn(&'a mut u8)",
        "-> [u8; 16]",
        "Item = fixture::S::Value",
    ] {
        assert!(identity.contains(part), "lost {part}: {identity}");
    }
}

#[test]
fn same_name_reexports_normalize_nested_trait_and_owner_types() {
    let arg = json!({"resolved_path": {"id": "arg", "path": "Arg", "args": null}});
    let args = json!({"angle_bracketed": {
        "args": [{"type": {"borrowed_ref": {
            "lifetime": "'a", "is_mutable": false, "type": {"tuple": [arg]}
        }}}],
        "constraints": []
    }});
    let root = json!({
        "root": "root",
        "paths": {
            "trait": {"path": ["fixture", "hidden", "T"]},
            "owner": {"path": ["fixture", "hidden", "S"]},
            "arg": {"path": ["fixture", "hidden", "Arg"]}
        },
        "index": {
            "root": {"id": "root", "name": "fixture", "inner": {"module": {
                "items": ["trait-export", "owner-export", "arg-export"]
            }}},
            "trait-export": {"visibility": "public", "inner": {"use": {
                "id": "trait", "name": "T", "is_glob": false
            }}},
            "owner-export": {"visibility": "public", "inner": {"use": {
                "id": "owner", "name": "S", "is_glob": false
            }}},
            "arg-export": {"visibility": "public", "inner": {"use": {
                "id": "arg", "name": "Arg", "is_glob": false
            }}},
            "trait": {"id": "trait", "name": "T", "inner": {"trait": {"items": []}}},
            "owner": {"id": "owner", "name": "S", "inner": {"struct": {
                "kind": "unit", "impls": ["implementation"]
            }}},
            "arg": {"id": "arg", "name": "Arg", "inner": {"struct": {
                "kind": "unit", "impls": []
            }}},
            "implementation": {"id": "implementation", "inner": {"impl": {
                "trait": {"id": "trait", "path": "T", "args": args},
                "for": {"resolved_path": {"id": "owner", "path": "S", "args": args}},
                "items": ["method"]
            }}},
            "method": {"id": "method", "name": "go", "inner": {"function": {}}}
        }
    });
    let map = rows_to_map(&extract(&root.to_string()));
    let owner = "fixture::S<&'a (fixture::Arg,)>";
    let trait_path = "fixture::T<&'a (fixture::Arg,)>";
    assert_eq!(
        map.get(&(owner.into(), "go".into())),
        Some(&Some(trait_path.into())),
    );
    let header = "impl<'a> fixture::T<&'a (fixture::Arg,)> for fixture::S<&'a (fixture::Arg,)>";
    let member = "pub fn fixture::S<&'a (fixture::Arg,)>::go(&self)";
    for lines in [
        [header.to_string(), member.to_string()],
        [member.to_string(), header.to_string()],
    ] {
        assert_eq!(
            crate::changelog::render(&lines, crate::model::Section::Added, "fixture", &map),
            vec![
                "- `impl<'a> T<&'a (fixture::Arg,)> for S<&'a (fixture::Arg,)>`:",
                "  - `go`"
            ],
        );
    }
    let items = crate::rustdoc::public_items(&root).items;
    assert!(items.contains_key(&format!("<{owner} as {trait_path}>::go")));
    assert!(!items.keys().any(|path| path.contains("hidden")));
}

#[test]
fn exported_type_formatting_does_not_rewrite_const_literals() {
    let root = json!({
        "root": "root",
        "paths": {"arg": {"path": ["fixture", "hidden", "Arg"]}},
        "index": {
            "root": {"id": "root", "name": "fixture", "inner": {"module": {"items": ["export"]}}},
            "export": {"visibility": "public", "inner": {"use": {
                "id": "arg", "name": "Arg", "is_glob": false
            }}},
            "arg": {"id": "arg", "name": "Arg"}
        }
    });
    let public = crate::rustdoc::public_items(&root);
    let ty = json!({"resolved_path": {"path": "dependency::Array", "args": {
        "angle_bracketed": {
            "args": [
                {"type": {"resolved_path": {"id": "arg", "path": "Arg"}}},
                {"const": {"expr": "{ \"fixture::hidden::Arg\".len() }"}}
            ],
            "constraints": []
        }
    }}});
    assert_eq!(
        public_type_name(&root, &ty, &public).as_deref(),
        Some("dependency::Array<fixture::Arg, { \"fixture::hidden::Arg\".len() }>"),
    );
}

fn export_preference_fixture() -> Value {
    json!({
        "root": "root",
        "paths": {
            "arg": {"path": ["fixture", "hidden", "Arg"]},
            "owner": {"path": ["fixture", "S"]},
            "trait": {"path": ["fixture", "T"]}
        },
        "index": {
            "root": {"id": "root", "name": "fixture", "inner": {"module": {
                "items": ["a", "z", "owner", "trait"]
            }}},
            "a": {"id": "a", "name": "a", "visibility": "public",
                "inner": {"module": {"items": ["nested"]}}},
            "nested": {"id": "nested", "name": "nested", "visibility": "public",
                "inner": {"module": {"items": ["deep"]}}},
            "deep": {"visibility": "public", "inner": {"use": {
                "id": "arg", "name": "Arg", "is_glob": false
            }}},
            "z": {"id": "z", "name": "very_long_export_module", "visibility": "public",
                "inner": {"module": {"items": ["shallow"]}}},
            "shallow": {"visibility": "public", "inner": {"use": {
                "id": "arg", "name": "Arg", "is_glob": false
            }}},
            "arg": {"id": "arg", "name": "Arg", "inner": {"struct": {"kind": "unit", "impls": []}}},
            "owner": {"id": "owner", "name": "S", "visibility": "public",
                "inner": {"struct": {"kind": "unit", "impls": ["implementation"]}}},
            "trait": {"id": "trait", "name": "T", "visibility": "public",
                "inner": {"trait": {"items": []}}},
            "implementation": {"inner": {"impl": {
                "for": {"resolved_path": {"id": "owner", "path": "S"}},
                "trait": {"id": "trait", "path": "T", "args": {"angle_bracketed": {
                    "args": [{"type": {"resolved_path": {"id": "arg", "path": "Arg"}}}],
                    "constraints": []
                }}},
                "items": ["method"]
            }}},
            "method": {"name": "go", "inner": {"function": {}}}
        }
    })
}

#[test]
fn competing_export_depths_use_path_segments_and_join_the_explicit_impl() {
    let root = export_preference_fixture();
    let map = rows_to_map(&extract(&root.to_string()));
    let trait_path = "fixture::T<fixture::very_long_export_module::Arg>";
    assert_eq!(
        map.get(&("fixture::S".into(), "go".into())),
        Some(&Some(trait_path.into())),
    );
    assert_eq!(
        crate::changelog::render(
            &[
                format!("impl {trait_path} for fixture::S"),
                "pub fn fixture::S::go(&self)".into(),
            ],
            crate::model::Section::Added,
            "fixture",
            &map,
        ),
        vec![
            "- `impl T<fixture::very_long_export_module::Arg> for S`:",
            "  - `go`"
        ],
    );
}

#[test]
fn export_ties_follow_source_order_and_unrenamed_globs_beat_shorter_aliases() {
    let mut root = export_preference_fixture();
    root["index"]["a"]["inner"]["module"]["items"] = json!(["deep"]);
    for order in [
        json!(["z", "a", "owner", "trait"]),
        json!(["a", "z", "owner", "trait"]),
    ] {
        root["index"]["root"]["inner"]["module"]["items"] = order;
        let expected = if root["index"]["root"]["inner"]["module"]["items"][0] == "z" {
            "fixture::very_long_export_module::Arg"
        } else {
            "fixture::a::Arg"
        };
        let public = crate::rustdoc::public_items(&root);
        assert_eq!(
            public_path_name(&root, &json!({"id": "arg", "path": "Arg"}), &public).as_deref(),
            Some(expected)
        );
    }
    root["index"]["deep"]["inner"]["use"]["id"] = json!("hidden");
    root["index"]["deep"]["inner"]["use"]["is_glob"] = json!(true);
    root["index"]["hidden"] = json!({"id": "hidden", "name": "hidden",
        "inner": {"module": {"items": ["arg"]}}});
    root["index"]["arg"]["visibility"] = json!("public");
    root["index"]["a"]["inner"]["module"]["items"] = json!(["nested"]);
    let public = crate::rustdoc::public_items(&root);
    assert_eq!(
        public_path_name(&root, &json!({"id": "arg", "path": "Arg"}), &public).as_deref(),
        Some("fixture::a::nested::Arg"),
    );
}

#[test]
fn impl_comparison_keeps_foreign_and_unexported_references_canonical() {
    let root = json!({
        "root": 0,
        "paths": {
            "1": {"crate_id": 1, "path": ["dependency", "Owner"]},
            "2": {"crate_id": 0, "path": ["fixture", "hidden", "Private"]}
        },
        "index": {
            "0": {"id": 0, "crate_id": 0, "name": "fixture",
                "inner": {"module": {"items": [3]}}},
            "1": {"id": 1, "crate_id": 1, "name": "Owner"},
            "2": {"id": 2, "crate_id": 0, "name": "Private"},
            "3": {"id": 3, "crate_id": 0, "visibility": "public",
                "inner": {"use": {"id": 1, "name": "Alias", "is_glob": false}}}
        }
    });
    for (id, canonical) in [(1, "dependency::Owner"), (2, "fixture::hidden::Private")] {
        let implementation = json!({
            "trait": null, "for": {"resolved_path": {"id": id, "path": "Owner", "args": null}}
        });
        let old = impl_identity(
            &root,
            &implementation,
            &crate::rustdoc::public_exports(&root),
        )
        .unwrap();
        assert_eq!(old.shape, canonical);
        let mut moved = root.clone();
        moved["paths"][id.to_string()]["path"] = json!(["other", "Owner"]);
        let new = impl_identity(
            &moved,
            &implementation,
            &crate::rustdoc::public_exports(&moved),
        )
        .unwrap();
        assert!(!old.matches(&new), "empty public slots are not wildcards");
        assert!(old.matches(&old));
    }
}
