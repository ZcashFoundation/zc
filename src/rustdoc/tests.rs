use super::*;
use serde_json::json;

fn fixture(items: Vec<Value>, exports: &[u64]) -> Value {
    let mut root = json!({"root": 0, "paths": {}, "index": {"0": {
        "id": 0, "crate_id": 0, "name": "demo", "visibility": "public",
        "inner": {"module": {"items": exports}}
    }}});
    for item in items {
        let id = id_text(&item["id"]).unwrap();
        root["index"][id] = item;
    }
    root
}

fn named(id: u64, name: &str, visibility: &str, inner: Value) -> Value {
    json!({"id": id, "crate_id": 0, "name": name, "visibility": visibility, "inner": inner})
}

#[test]
fn follows_exports_and_implicit_members_without_private_leaks() {
    let root = fixture(
        vec![
            named(1, "hidden", "default", json!({"module": {"items": [2, 8]}})),
            named(
                2,
                "Record",
                "public",
                json!({"struct": {"kind": {
            "plain": {"fields": [3, 4], "has_stripped_fields": false}
        }, "impls": [10]}}),
            ),
            named(
                3,
                "visible",
                "public",
                json!({"struct_field": {"primitive": "u8"}}),
            ),
            named(
                4,
                "secret",
                "default",
                json!({"struct_field": {"primitive": "u8"}}),
            ),
            named(
                5,
                "export",
                "public",
                json!({"use": {"id": 2, "name": "Alias", "is_glob": false}}),
            ),
            named(
                6,
                "State",
                "public",
                json!({"enum": {"variants": [7], "impls": []}}),
            ),
            named(
                7,
                "Ready",
                "default",
                json!({"variant": {"kind": {"struct": {"fields": [9]}}}}),
            ),
            named(8, "unrelated", "public", json!({"function": {}})),
            named(
                9,
                "payload",
                "default",
                json!({"struct_field": {"primitive": "u8"}}),
            ),
            named(
                10,
                "impl",
                "default",
                json!({"impl": {"trait": null, "items": [11, 12]}}),
            ),
            named(
                11,
                "VALUE",
                "public",
                json!({"assoc_const": {"value": "1"}}),
            ),
            named(12, "secret_method", "default", json!({"function": {}})),
            named(13, "Trait", "public", json!({"trait": {"items": [14]}})),
            named(
                14,
                "method",
                "default",
                json!({"function": {"has_body": false}}),
            ),
        ],
        &[1, 5, 6, 13],
    );
    let items = public_items(&root).items;
    for path in [
        "demo::Alias",
        "demo::Alias::visible",
        "demo::Alias::VALUE",
        "demo::State::Ready",
        "demo::State::Ready::payload",
        "demo::Trait::method",
    ] {
        assert!(items.contains_key(path), "missing {path}");
    }
    for path in [
        "demo::hidden",
        "demo::Record",
        "demo::Alias::secret",
        "demo::Alias::secret_method",
        "demo::hidden::unrelated",
    ] {
        assert!(!items.contains_key(path), "leaked {path}");
    }
}

#[test]
fn handles_glob_cycles_and_preserves_foreign_reexport_provenance() {
    let mut root = fixture(
        vec![
            named(
                1,
                "hidden",
                "default",
                json!({"module": {"items": [2, 3, 4]}}),
            ),
            named(
                2,
                "glob",
                "public",
                json!({"use": {"id": 1, "name": "*", "is_glob": true}}),
            ),
            named(
                3,
                "Foreign",
                "public",
                json!({"struct": {"kind": "unit", "impls": []}}),
            ),
            named(
                4,
                "summary",
                "public",
                json!({"use": {"id": 99, "name": "External", "is_glob": false}}),
            ),
        ],
        &[2],
    );
    root["index"]["3"]["crate_id"] = json!(7);
    root["paths"]["99"] = json!({"crate_id": 8, "path": ["other", "External"], "kind": "struct"});
    let items = public_items(&root).items;
    assert_eq!(items["demo::Foreign"]["crate_id"], 7);
    assert_eq!(items["demo::External"]["crate_id"], 8);
    assert!(std::ptr::eq(items["demo::Foreign"], &root["index"]["3"]));
}

#[test]
fn retains_trait_impl_member_identity_and_visibility() {
    let mut root = fixture(
        vec![
            named(
                1,
                "Owner",
                "public",
                json!({"struct": {"kind": "unit", "impls": [3, 4]}}),
            ),
            named(2, "PublicTrait", "public", json!({"trait": {"items": []}})),
            named(
                3,
                "impl",
                "default",
                json!({"impl": {
                    "trait": {"id": 2, "path": "PublicTrait", "args": null}, "items": [5]
                }}),
            ),
            named(
                4,
                "impl",
                "default",
                json!({"impl": {
                    "trait": {"id": 6, "path": "PrivateTrait", "args": null}, "items": [7]
                }}),
            ),
            named(5, "method", "default", json!({"function": {}})),
            named(6, "PrivateTrait", "public", json!({"trait": {"items": []}})),
            named(
                7,
                "private_trait_method",
                "default",
                json!({"function": {}}),
            ),
        ],
        &[1, 2],
    );
    root["paths"]["2"] = json!({"crate_id": 0, "path": ["demo", "PublicTrait"], "kind": "trait"});
    assert!(public_items(&root)
        .items
        .contains_key("<demo::Owner as demo::PublicTrait>::method"));
    assert!(!public_items(&root)
        .items
        .keys()
        .any(|path| path.contains("private_trait_method")));
}

#[test]
fn rejects_invalid_or_unrelated_json() {
    assert!(parse_json("{").is_err());
    assert!(parse_json("{}").is_err());
}

#[test]
fn preserves_foreign_globs_and_empty_trait_impl_provenance() {
    let mut root = fixture(
        vec![
            named(
                1,
                "Owner",
                "public",
                json!({"struct": {"kind": "unit", "impls": [2]}}),
            ),
            named(
                2,
                "impl",
                "default",
                json!({"impl": {
                    "trait": {"id": 99, "path": "Trait", "args": null}, "items": []
                }}),
            ),
            named(
                3,
                "glob",
                "public",
                json!({"use": {
                    "id": 98, "source": "dependency::module", "name": "*", "is_glob": true
                }}),
            ),
        ],
        &[1, 3],
    );
    root["paths"]["99"] = json!({"crate_id": 7, "path": ["dependency", "Trait"], "kind": "trait"});
    root["paths"]["98"] = json!({"crate_id": 8, "path": ["other", "module"], "kind": "module"});
    let items = public_items(&root).items;
    assert!(std::ptr::eq(
        items["<demo::Owner as dependency::Trait>"],
        &root["index"]["2"]
    ));
    assert_eq!(items["demo::{dependency::module::*}"]["crate_id"], 8);
}

#[test]
fn keeps_distinct_generic_trait_implementations() {
    let mut root = fixture(
        vec![
            named(
                1,
                "Owner",
                "public",
                json!({"struct": {"kind": "unit", "impls": [2, 3]}}),
            ),
            named(
                2,
                "impl",
                "default",
                json!({"impl": {
                    "trait": {"id": 99, "path": "Trait", "args": {"angle_bracketed": {
                        "args": [{"type": {"primitive": "u8"}}], "constraints": []
                    }}}, "items": [4]
                }}),
            ),
            named(
                3,
                "impl",
                "default",
                json!({"impl": {
                    "trait": {"id": 99, "path": "Trait", "args": {"angle_bracketed": {
                        "args": [{"type": {"primitive": "u16"}}], "constraints": []
                    }}}, "items": [5]
                }}),
            ),
            named(
                4,
                "VALUE",
                "default",
                json!({"assoc_const": {"value": "8"}}),
            ),
            named(
                5,
                "VALUE",
                "default",
                json!({"assoc_const": {"value": "16"}}),
            ),
        ],
        &[1],
    );
    root["paths"]["99"] = json!({"crate_id": 7, "path": ["dependency", "Trait"], "kind": "trait"});
    let items = public_items(&root).items;
    assert_eq!(
        items["<demo::Owner as dependency::Trait<u8>>::VALUE"]["id"],
        4
    );
    assert_eq!(
        items["<demo::Owner as dependency::Trait<u16>>::VALUE"]["id"],
        5
    );
}

#[test]
fn preserves_specialized_reference_owners_through_reexports() {
    let owner = json!({"resolved_path": {"id": 1, "path": "Owner", "args": {
        "angle_bracketed": {"args": [{"type": {"primitive": "u8"}}], "constraints": []}
    }}});
    let mut root = fixture(
        vec![
            named(
                1,
                "Owner",
                "public",
                json!({"struct": {"kind": "unit", "impls": [2, 3]}}),
            ),
            named(
                2,
                "impl",
                "default",
                json!({"impl": {
                    "trait": {"id": 99, "path": "Trait", "args": null}, "items": [4],
                    "for": {"borrowed_ref": {"lifetime": null, "is_mutable": false, "type": owner.clone()}}
                }}),
            ),
            named(
                3,
                "impl",
                "default",
                json!({"impl": {
                    "trait": null, "items": [5], "for": owner
                }}),
            ),
            named(4, "method", "default", json!({"function": {}})),
            named(5, "VALUE", "public", json!({"assoc_const": {"value": "8"}})),
            named(
                8,
                "export",
                "public",
                json!({"use": {"id": 1, "name": "Alias", "is_glob": false}}),
            ),
        ],
        &[8],
    );
    root["paths"]["1"] =
        json!({"crate_id": 0, "path": ["demo", "hidden", "Owner"], "kind": "struct"});
    root["paths"]["99"] = json!({"crate_id": 7, "path": ["dependency", "Trait"], "kind": "trait"});
    let items = public_items(&root).items;
    assert!(items.contains_key("<&demo::Alias<u8> as dependency::Trait>::method"));
    assert!(items.contains_key("demo::Alias<u8>::VALUE"));
}

#[test]
fn added_trait_aliases_preserve_existing_impl_override_values() {
    for private_definition in [false, true] {
        let mut base = fixture(
            vec![
                named(
                    1,
                    "S",
                    "public",
                    json!({"struct": {"kind": "unit", "impls": [2]}}),
                ),
                named(
                    2,
                    "impl",
                    "default",
                    json!({"impl": {
                        "trait": {"id": 3, "path": "T", "args": null},
                        "for": {"resolved_path": {"id": 1, "path": "S", "args": null}},
                        "items": [5]
                    }}),
                ),
                named(3, "T", "public", json!({"trait": {"items": [4]}})),
                named(
                    4,
                    "LIMIT",
                    "default",
                    json!({"assoc_const": {
                        "type": {"primitive": "u8"}, "value": "1"
                    }}),
                ),
                named(
                    5,
                    "LIMIT",
                    "default",
                    json!({"assoc_const": {
                        "type": {"primitive": "u8"}, "value": "2"
                    }}),
                ),
                named(
                    6,
                    "export",
                    "public",
                    json!({"use": {
                        "id": 3, "name": "T", "is_glob": false
                    }}),
                ),
                named(
                    7,
                    "export",
                    "public",
                    json!({"use": {
                        "id": 3, "name": "B", "is_glob": false
                    }}),
                ),
                named(
                    8,
                    "export",
                    "public",
                    json!({"use": {
                        "id": 3, "name": "A", "is_glob": false
                    }}),
                ),
            ],
            if private_definition {
                &[1, 6, 7]
            } else {
                &[1, 3]
            },
        );
        base["paths"]["1"] = json!({"crate_id": 0, "path": ["demo", "S"]});
        base["paths"]["3"] = if private_definition {
            json!({"crate_id": 0, "path": ["demo", "hidden", "T"]})
        } else {
            json!({"crate_id": 0, "path": ["demo", "T"]})
        };
        let mut head = base.clone();
        head["index"]["0"]["inner"]["module"]["items"]
            .as_array_mut()
            .unwrap()
            .push(json!(8));
        head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
        let before = public_items(&base).items;
        let after = public_items(&head).items;
        for path in ["<demo::S as demo::T>::LIMIT", "<demo::S as demo::B>::LIMIT"] {
            if !private_definition && path.contains("demo::B") {
                continue;
            }
            assert_eq!(before[path]["inner"]["assoc_const"]["value"], "2");
            assert_eq!(after[path]["inner"]["assoc_const"]["value"], "3");
        }
        assert_eq!(
            after["<demo::S as demo::A>::LIMIT"]["inner"]["assoc_const"]["value"],
            "3"
        );
        assert_eq!(
            after["demo::T::LIMIT"]["inner"]["assoc_const"]["value"],
            "1"
        );
        if !private_definition {
            assert_eq!(
                crate::traitmap::public_path_name(
                    &head,
                    &head["index"]["2"]["inner"]["impl"]["trait"],
                    &public_items(&head),
                )
                .as_deref(),
                Some("demo::T"),
            );
        }
        assert!(!after.keys().any(|path| path.contains("hidden")));
    }
}

#[test]
fn foreign_owner_impls_share_headers_members_defaults_and_private_boundaries() {
    let foreign_owner = json!({"resolved_path": {"id": 90, "path": "Vec", "args": {
        "angle_bracketed": {"args": [{"type": {"primitive": "u8"}}], "constraints": []}
    }}});
    let mut root = fixture(
        vec![
            named(
                1,
                "T",
                "public",
                json!({"trait": {
                    "items": [2, 3, 4], "implementations": [5, 8, 9, 10, 11]
                }}),
            ),
            named(2, "Out", "default", json!({"assoc_type": {"type": null}})),
            named(3, "C", "default", json!({"assoc_const": {"value": null}})),
            named(
                4,
                "DEFAULT",
                "default",
                json!({"assoc_const": {"value": "7"}}),
            ),
            named(
                5,
                "impl",
                "default",
                json!({"impl": {
                    "trait": {"id": 1, "path": "T", "args": null},
                    "for": foreign_owner, "items": [12, 13],
                    "generics": {"params": [], "where_predicates": [{
                        "bound_predicate": {"type": {"resolved_path": {
                            "id": 91, "path": "S", "args": null
                        }}, "bounds": [], "generic_params": []}
                    }]}
                }}),
            ),
            named(6, "PrivateTrait", "public", json!({"trait": {"items": []}})),
            named(
                7,
                "PrivateOwner",
                "public",
                json!({"struct": {"kind": "unit", "impls": []}}),
            ),
            named(
                12,
                "Out",
                "default",
                json!({"assoc_type": {"type": {
                    "resolved_path": {"id": 91, "path": "S", "args": null}
                }}}),
            ),
            named(13, "C", "default", json!({"assoc_const": {"value": "2"}})),
        ],
        &[1],
    );
    root["paths"]["90"] = json!({"crate_id": 1, "path": ["alloc", "vec", "Vec"]});
    root["paths"]["91"] = json!({"crate_id": 2, "path": ["bar", "S"]});
    for id in [8, 9, 10, 11, 14] {
        root["index"][id.to_string()] = root["index"]["5"].clone();
        root["index"][id.to_string()]["id"] = json!(id);
    }
    root["index"]["8"]["inner"]["impl"]["for"] =
        json!({"resolved_path": {"id": 7, "path": "PrivateOwner", "args": null}});
    root["index"]["9"]["inner"]["impl"]["for"]["resolved_path"]["args"]["angle_bracketed"]
        ["args"][0] = json!({"type": {"resolved_path": {
        "id": 7, "path": "PrivateOwner", "args": null
    }}});
    root["index"]["10"]["inner"]["impl"]["is_negative"] = json!(true);
    root["index"]["11"]["inner"]["impl"]["trait"]["id"] = json!(6);
    root["index"]["13"]["docs"] = json!("override docs");
    root["index"]["4"]["docs"] = json!("default docs");

    let exports = public_exports(&root);
    let shared = exports.items.keys().cloned().collect();
    for public in [
        public_items(&root),
        public_items_for_comparison(&root, exports, Some(&shared)),
    ] {
        let items = public.items;
        let owner = "<alloc::vec::Vec<u8> as demo::T>";
        assert!(std::ptr::eq(items[owner], &root["index"]["5"]));
        assert!(std::ptr::eq(
            items[&format!("{owner}::Out")],
            &root["index"]["12"]
        ));
        assert_eq!(items[&format!("{owner}::C")]["docs"], "override docs");
        assert_eq!(items[&format!("{owner}::DEFAULT")]["docs"], "default docs");
        assert_eq!(
            items
                .values()
                .filter_map(|item| item.pointer("/inner/impl"))
                .count(),
            1,
            "private owners/arguments/traits, negative impls and unrelated impls stay out"
        );
    }
}
