use super::*;
use serde_json::json;

fn compare(base: &[IndexRow], head: &[IndexRow]) -> (Vec<ValueChange>, Vec<DocChange>) {
    super::compare(base, head).unwrap()
}

fn fixture() -> Value {
    let mut root = json!({"root": 0, "index": {}, "paths": {}});
    let items = [
        json!({"id": 0, "name": "demo", "visibility": "public",
            "inner": {"module": {"items": [1, 4, 6, 12, 13]}}}),
        json!({"id": 1, "name": "State", "visibility": "public",
            "inner": {"enum": {"variants": [2], "impls": []}}}),
        json!({"id": 2, "name": "Ready", "visibility": "default", "docs": "old variant",
            "inner": {"variant": {"kind": "plain"}}}),
        json!({"id": 4, "name": "Trait", "visibility": "public",
            "inner": {"trait": {"items": [5]}}}),
        json!({"id": 5, "name": "method", "visibility": "default", "docs": null,
            "inner": {"function": {"has_body": false}}}),
        json!({"id": 6, "name": "Record", "visibility": "public", "docs": "record docs",
            "inner": {"struct": {"kind": {"plain": {"fields": [7, 8]}}, "impls": [9]}}}),
        json!({"id": 7, "name": "field", "visibility": "public", "docs": "old field",
            "inner": {"struct_field": {"primitive": "u8"}}}),
        json!({"id": 8, "name": "private", "visibility": "default", "docs": "private",
            "inner": {"struct_field": {"primitive": "u8"}}}),
        json!({"id": 9, "name": null, "visibility": "default",
            "inner": {"impl": {"trait": null, "items": [10, 11]}}}),
        json!({"id": 10, "name": "LIMIT", "visibility": "public",
            "inner": {"assoc_const": {"type": {"primitive": "u64"}, "value": "1"}}}),
        json!({"id": 11, "name": "PRIVATE", "visibility": "default",
            "inner": {"assoc_const": {"type": {"primitive": "u64"}, "value": "1"}}}),
        json!({"id": 12, "name": "GLOBAL", "visibility": "public",
            "inner": {"constant": {"type": {"primitive": "u8"}, "const": {"value": "2"}}}}),
        json!({"id": 13, "name": "STATIC", "visibility": "public",
            "inner": {"static": {"type": {"primitive": "u32"}, "expr": "3"}}}),
    ];
    for mut item in items {
        item["crate_id"] = json!(0);
        let id = item["id"].to_string();
        root["index"][id] = item;
    }
    root
}

#[test]
fn finds_associated_values_and_all_surviving_documentation_changes() {
    let base = fixture();
    let mut head = base.clone();
    head["index"]["10"]["inner"]["assoc_const"]["value"] = json!("10");
    head["index"]["11"]["inner"]["assoc_const"]["value"] = json!("11");
    head["index"]["12"]["inner"]["constant"]["const"]["value"] = json!("20");
    head["index"]["13"]["inner"]["static"]["expr"] = json!("30");
    head["index"]["2"]["docs"] = json!("new variant");
    head["index"]["5"]["docs"] = json!("new method docs");
    head["index"]["6"]["docs"] = Value::Null;
    head["index"]["7"]["docs"] = json!("new field");
    head["index"]["8"]["docs"] = json!("still private");
    let (values, docs) = compare(
        &extract(&base.to_string(), "demo-package").unwrap(),
        &extract(&head.to_string(), "demo-package").unwrap(),
    );
    assert_eq!(
        values
            .iter()
            .map(|row| row.path.as_str())
            .collect::<Vec<_>>(),
        ["demo::GLOBAL", "demo::Record::LIMIT", "demo::STATIC"]
    );
    assert_eq!(
        (
            values[1].ty.as_str(),
            values[1].old.as_str(),
            values[1].new.as_str()
        ),
        ("u64", "1", "10")
    );
    assert_eq!(
        docs.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(),
        [
            "demo::Record",
            "demo::Record::field",
            "demo::State::Ready",
            "demo::Trait::method"
        ]
    );
    assert!(values.iter().all(|row| row.crate_name == "demo-package"));
}

#[test]
fn ignores_documentation_on_wholly_new_or_removed_items() {
    let base = fixture();
    let mut head = base.clone();
    head["index"]["0"]["inner"]["module"]["items"] = json!([1, 4, 6, 12, 14]);
    head["index"]["14"] = json!({"id": 14, "name": "ADDED", "visibility": "public",
        "docs": "new item", "inner": {"function": {}}});
    let (values, docs) = compare(
        &extract(&base.to_string(), "demo").unwrap(),
        &extract(&head.to_string(), "demo").unwrap(),
    );
    assert!(values.is_empty());
    assert!(docs.is_empty());
}

#[test]
fn adding_and_removing_overrides_compares_inherited_values_at_full_impl_paths() {
    let mut base = fixture();
    base["index"]["4"]["inner"]["trait"]["items"] = json!([5, 20]);
    base["index"]["20"] = json!({"id": 20, "crate_id": 0, "name": "LIMIT",
        "visibility": "default",
        "inner": {"assoc_const": {"type": {"primitive": "u8"}, "value": "1"}}});
    base["index"]["6"]["inner"]["struct"]["impls"] = json!([9, 21, 22, 24]);
    for (id, argument) in [(21, "u8"), (22, "u16")] {
        base["index"][id.to_string()] = json!({"id": id, "crate_id": 0,
        "visibility": "default", "inner": {"impl": {
            "trait": {"id": 4, "path": "Trait", "args": {"angle_bracketed": {
                "args": [{"type": {"primitive": argument}}], "constraints": []
            }}},
            "for": {"resolved_path": {"id": 6, "path": "Record", "args": {
                "angle_bracketed": {"args": [{"type": {"primitive": "u8"}}],
                    "constraints": []}
            }}}, "items": []
        }}});
    }
    base["index"]["23"] = json!({"id": 23, "crate_id": 0, "name": "LIMIT",
        "visibility": "default",
        "inner": {"assoc_const": {"type": {"primitive": "u8"}, "value": "2"}}});
    base["index"]["24"] = base["index"]["21"].clone();
    base["index"]["24"]["id"] = json!(24);
    base["index"]["24"]["inner"]["impl"]["for"]["resolved_path"]["args"]["angle_bracketed"]
        ["args"][0]["type"]["primitive"] = json!("u16");
    base["paths"]["4"] = json!({"crate_id": 0, "path": ["demo", "Trait"], "kind": "trait"});
    base["paths"]["6"] = json!({"crate_id": 0, "path": ["demo", "Record"], "kind": "struct"});

    let mut head = base.clone();
    head["index"]["21"]["inner"]["impl"]["items"] = json!([23]);
    let inherited = extract(&base.to_string(), "demo-package").unwrap();
    let overridden = extract(&head.to_string(), "demo-package").unwrap();
    for (before, after, old, new) in [
        (&inherited, &overridden, "1", "2"),
        (&overridden, &inherited, "2", "1"),
    ] {
        let (values, _) = compare(before, after);
        assert_eq!(values.len(), 1);
        assert_eq!(
            values[0].path,
            "<demo::Record<u8> as demo::Trait<u8>>::LIMIT"
        );
        assert_eq!((values[0].old.as_str(), values[0].new.as_str()), (old, new));
        assert_eq!(values[0].crate_name, "demo-package");
    }
}

#[test]
fn malformed_rustdoc_is_an_error() {
    assert!(extract("not JSON", "demo").is_err());
    assert!(extract("{}", "demo").is_err());
}

fn nested_impl_fixture(generic_owner: bool) -> Value {
    let arg = json!({"type": {"resolved_path": {"id": 8, "path": "Arg", "args": null}}});
    let args = json!({"angle_bracketed": {"args": [arg], "constraints": []}});
    let mut root = json!({
        "root": 0,
        "paths": {
            "1": {"crate_id": 0, "path": ["demo", "S"]},
            "3": {"crate_id": 0, "path": ["demo", "T"]},
            "8": {"crate_id": 0, "path": ["demo", "hidden", "Arg"]}
        },
        "index": {
            "0": {"id": 0, "crate_id": 0, "name": "demo", "visibility": "public",
                "inner": {"module": {"items": [1, 3, 6]}}},
            "1": {"id": 1, "crate_id": 0, "name": "S", "visibility": "public",
                "inner": {"struct": {"kind": "unit", "impls": [2]}}},
            "2": {"id": 2, "crate_id": 0, "visibility": "default", "inner": {"impl": {
                "trait": {"id": 3, "path": "T", "args": args},
                "for": {"resolved_path": {"id": 1, "path": "S", "args": null}},
                "items": [5]
            }}},
            "3": {"id": 3, "crate_id": 0, "name": "T", "visibility": "public",
                "inner": {"trait": {"items": [4]}}},
            "4": {"id": 4, "crate_id": 0, "name": "LIMIT", "visibility": "default",
                "inner": {"assoc_const": {"type": {"primitive": "u8"}, "value": "1"}}},
            "5": {"id": 5, "crate_id": 0, "name": "LIMIT", "visibility": "default",
                "docs": "old override",
                "inner": {"assoc_const": {"type": {"primitive": "u8"}, "value": "2"}}},
            "6": {"id": 6, "crate_id": 0, "name": "z", "visibility": "public",
                "inner": {"module": {"items": [7]}}},
            "7": {"id": 7, "crate_id": 0, "visibility": "public", "inner": {"use": {
                "id": 8, "name": "Arg", "is_glob": false
            }}},
            "8": {"id": 8, "crate_id": 0, "name": "Arg", "visibility": "public",
                "inner": {"struct": {"kind": "unit", "impls": []}}},
            "9": {"id": 9, "crate_id": 0, "name": "a", "visibility": "public",
                "inner": {"module": {"items": [10]}}},
            "10": {"id": 10, "crate_id": 0, "name": "nested", "visibility": "public",
                "inner": {"module": {"items": [11]}}},
            "11": {"id": 11, "crate_id": 0, "visibility": "public", "inner": {"use": {
                "id": 8, "name": "A", "is_glob": false
            }}}
        }
    });
    if generic_owner {
        root["index"]["2"]["inner"]["impl"]["for"]["resolved_path"]["args"] = args;
    }
    root
}

#[test]
fn nested_alias_additions_do_not_hide_impl_override_or_documentation_changes() {
    for generic_owner in [false, true] {
        let base = nested_impl_fixture(generic_owner);
        let mut head = base.clone();
        head["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 6, 9]);
        let (values, docs) = compare(
            &extract(&base.to_string(), "demo").unwrap(),
            &extract(&head.to_string(), "demo").unwrap(),
        );
        assert!(values.is_empty());
        assert!(docs.is_empty());

        head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
        head["index"]["5"]["docs"] = json!("new override");
        let (values, docs) = compare(
            &extract(&base.to_string(), "demo").unwrap(),
            &extract(&head.to_string(), "demo").unwrap(),
        );
        let owner = if generic_owner {
            "demo::S<demo::z::Arg>"
        } else {
            "demo::S"
        };
        let path = format!("<{owner} as demo::T<demo::z::Arg>>::LIMIT");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].path, path);
        assert_eq!((values[0].old.as_str(), values[0].new.as_str()), ("2", "3"));
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].path, path);
    }
}

#[test]
fn stable_exported_paths_match_impl_values_across_private_definition_moves() {
    let base = nested_impl_fixture(true);
    let mut head = base.clone();
    head["paths"]["8"]["path"] = json!(["demo", "moved", "Arg"]);
    head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
    head["index"]["5"]["docs"] = json!("new override");
    let (values, docs) = compare(
        &extract(&base.to_string(), "demo").unwrap(),
        &extract(&head.to_string(), "demo").unwrap(),
    );
    assert_eq!(values.len(), 1);
    let path = "<demo::S<demo::z::Arg> as demo::T<demo::z::Arg>>::LIMIT";
    assert_eq!(values[0].path, path);
    assert_eq!((values[0].old.as_str(), values[0].new.as_str()), ("2", "3"));
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].path, path);
}

#[test]
fn const_literal_changes_remain_distinct_impl_identities_when_aliases_change() {
    let mut base = nested_impl_fixture(false);
    base["index"]["2"]["inner"]["impl"]["trait"]["args"]["angle_bracketed"]["args"]
        .as_array_mut()
        .unwrap()
        .push(json!({"const": {
            "expr": "{ \"demo::hidden::Arg\".len() }"
        }}));
    let mut head = base.clone();
    head["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 6, 9]);
    head["index"]["2"]["inner"]["impl"]["trait"]["args"]["angle_bracketed"]["args"][1] =
        json!({"const": {"expr": "{ \"demo::z::Arg\".len() }"}});
    head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
    let (before, after) = extract_pair(&base, &head, "demo");
    let (values, _) = compare(&before, &after);
    assert!(
        values.is_empty(),
        "different const arguments introduce a different impl"
    );
}

#[test]
fn retargeted_reexports_compare_the_surviving_public_values_and_docs() {
    let mut base = fixture();
    base["index"]["0"]["inner"]["module"]["items"] = json!([20, 21, 22]);
    base["index"]["6"]["name"] = json!("A");
    base["index"]["6"]["inner"]["struct"]["kind"] = json!("unit");
    base["index"]["9"]["inner"]["impl"]["for"] =
        json!({"resolved_path": {"id": 6, "path": "A", "args": null}});
    base["index"]["10"]["docs"] = json!("A constant");
    base["index"]["16"] = json!({"id": 16, "crate_id": 0, "name": "B",
        "visibility": "public", "inner": {"struct": {"kind": "unit", "impls": [19]}}});
    base["index"]["18"] = json!({"id": 18, "crate_id": 0, "name": "LIMIT",
        "visibility": "public", "docs": "B constant",
        "inner": {"assoc_const": {"type": {"primitive": "u64"}, "value": "2"}}});
    base["index"]["19"] = json!({"id": 19, "crate_id": 0, "name": null,
        "visibility": "default", "inner": {"impl": {"trait": null, "items": [18],
            "for": {"resolved_path": {"id": 16, "path": "B", "args": null}}}}});
    base["index"]["20"] = json!({"id": 20, "crate_id": 0, "name": "hidden",
        "visibility": "default", "inner": {"module": {"items": [6, 16]}}});
    for (id, name, target) in [(21, "First", 6), (22, "Second", 16)] {
        let source = if target == 6 {
            "hidden::A"
        } else {
            "hidden::B"
        };
        base["index"][id.to_string()] = json!({"id": id, "crate_id": 0, "name": name,
            "visibility": "public", "inner": {"use": {"name": name, "id": target,
                "source": source, "is_glob": false}}});
    }
    base["paths"]["6"] = json!({"path": ["demo", "hidden", "A"], "crate_id": 0, "kind": "struct"});
    base["paths"]["16"] = json!({"path": ["demo", "hidden", "B"], "crate_id": 0, "kind": "struct"});
    let mut head = base.clone();
    head["index"]["21"]["inner"]["use"]["id"] = json!(16);
    head["index"]["22"]["inner"]["use"]["id"] = json!(6);
    head["index"]["21"]["inner"]["use"]["source"] = json!("hidden::B");
    head["index"]["22"]["inner"]["use"]["source"] = json!("hidden::A");
    let (before, after) = extract_pair(&base, &head, "demo");
    let (values, docs) = compare(&before, &after);
    assert!(values
        .iter()
        .any(|value| value.path == "demo::First::LIMIT" && value.old == "1" && value.new == "2"));
    assert!(values
        .iter()
        .any(|value| value.path == "demo::Second::LIMIT" && value.old == "2" && value.new == "1"));
    assert!(docs.iter().any(|doc| doc.path == "demo::First::LIMIT"));
    assert!(docs.iter().any(|doc| doc.path == "demo::Second::LIMIT"));
}

#[test]
fn private_argument_moves_and_new_aliases_preserve_surviving_impl_values_and_docs() {
    for generic_owner in [false, true] {
        for shorter in [false, true] {
            let base = nested_impl_fixture(generic_owner);
            let mut head = base.clone();
            head["paths"]["8"]["path"] = json!(["demo", "moved", "Arg"]);
            head["index"]["0"]["inner"]["module"]["items"] = if shorter {
                json!([1, 3, 6, 11])
            } else {
                json!([1, 3, 6, 9])
            };
            let (before, after) = extract_pair(&base, &head, "demo");
            let (values, docs) = compare(&before, &after);
            assert!(values.is_empty());
            assert!(docs.is_empty());
            head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
            head["index"]["5"]["docs"] = json!("new override");
            let (before, after) = extract_pair(&base, &head, "demo");
            let (values, docs) = compare(&before, &after);
            assert_eq!(values.len(), 1);
            assert_eq!((values[0].old.as_str(), values[0].new.as_str()), ("2", "3"));
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0].path, values[0].path);
        }
    }
}

#[test]
fn surviving_argument_alias_compares_values_and_docs_after_partial_retarget_and_move() {
    for generic_owner in [false, true] {
        for moved in [false, true] {
            let mut base = nested_impl_fixture(generic_owner);
            base["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 7, 11]);
            base["index"]["7"]["inner"]["use"]["name"] = json!("A");
            base["index"]["11"]["inner"]["use"]["name"] = json!("B");
            base["index"]["12"] = json!({"id": 12, "crate_id": 0, "name": "Other",
                "visibility": "public", "inner": {"struct": {"kind": "unit", "impls": []}}});
            base["paths"]["12"] = json!({"crate_id": 0, "path": ["demo", "hidden", "Other"]});
            let mut head = base.clone();
            head["index"]["7"]["inner"]["use"]["id"] = json!(12);
            if moved {
                head["paths"]["8"]["path"] = json!(["demo", "moved", "Arg"]);
            }
            let (before, after) = extract_pair(&base, &head, "demo");
            let (values, docs) = compare(&before, &after);
            assert!(values.is_empty());
            assert!(docs.is_empty());

            head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
            head["index"]["5"]["docs"] = json!("new override");
            let (before, after) = extract_pair(&base, &head, "demo");
            let (values, docs) = compare(&before, &after);
            let owner = if generic_owner {
                "demo::S<demo::B>"
            } else {
                "demo::S"
            };
            let path = format!("<{owner} as demo::T<demo::B>>::LIMIT");
            assert_eq!(values.len(), 1);
            assert_eq!(values[0].path, path);
            assert_eq!((values[0].old.as_str(), values[0].new.as_str()), ("2", "3"));
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0].path, path);
        }
    }
}

#[test]
fn impl_header_docs_survive_a_changed_preferred_argument_alias() {
    let mut base = nested_impl_fixture(false);
    base["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 7, 11]);
    base["index"]["7"]["inner"]["use"]["name"] = json!("A");
    base["index"]["11"]["inner"]["use"]["name"] = json!("B");
    base["index"]["2"]["docs"] = json!("old impl");
    let mut head = base.clone();
    head["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 11, 7]);
    head["index"]["2"]["docs"] = json!("new impl");
    let (before, after) = extract_pair(&base, &head, "demo");
    let (values, docs) = compare(&before, &after);
    assert!(values.is_empty());
    assert_eq!(docs.len(), 1);
}

#[test]
fn canonical_impl_origin_wins_over_colliding_public_slots() {
    let mut base = nested_impl_fixture(false);
    base["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 7, 11, 13]);
    base["index"]["7"]["inner"]["use"]["name"] = json!("A");
    base["index"]["11"]["inner"]["use"]["name"] = json!("B");
    base["index"]["5"]["inner"]["assoc_const"]["value"] = json!("1");
    base["index"]["5"]["docs"] = json!("first");
    base["index"]["12"] = json!({"id": 12, "crate_id": 0, "name": "Other",
        "visibility": "public", "inner": {"struct": {"kind": "unit", "impls": []}}});
    base["paths"]["12"] = json!({"crate_id": 0, "path": ["demo", "hidden", "Other"]});
    base["index"]["13"] = json!({"id": 13, "crate_id": 0, "visibility": "public",
        "inner": {"use": {"id": 12, "name": "C", "is_glob": false}}});
    base["index"]["14"] = base["index"]["2"].clone();
    base["index"]["14"]["id"] = json!(14);
    base["index"]["14"]["inner"]["impl"]["items"] = json!([15]);
    base["index"]["14"]["inner"]["impl"]["trait"]["args"]["angle_bracketed"]["args"][0] =
        json!({"type": {"resolved_path": {"id": 12, "path": "Other", "args": null}}});
    base["index"]["15"] = base["index"]["5"].clone();
    base["index"]["15"]["id"] = json!(15);
    base["index"]["15"]["inner"]["assoc_const"]["value"] = json!("2");
    base["index"]["15"]["docs"] = json!("second");
    base["index"]["1"]["inner"]["struct"]["impls"] = json!([2, 14]);
    let mut head = base.clone();
    head["index"]["16"] = json!({"id": 16, "crate_id": 0, "visibility": "public",
        "inner": {"use": {"id": 12, "name": "E", "is_glob": false}}});
    head["index"]["0"]["inner"]["module"]["items"] = json!([16, 1, 3, 7, 11, 13]);
    head["index"]["11"]["inner"]["use"]["id"] = json!(12);
    head["index"]["15"]["inner"]["assoc_const"]["value"] = json!("1");
    head["index"]["15"]["docs"] = json!("first");
    let (before, after) = extract_pair(&base, &head, "demo");
    let (values, docs) = compare(&before, &after);
    assert_eq!(values.len(), 1);
    assert_eq!((values[0].old.as_str(), values[0].new.as_str()), ("2", "1"));
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].path, values[0].path);

    base["index"]["7"]["inner"]["use"]["name"] = json!("Z");
    base["index"]["11"]["inner"]["use"]["id"] = json!(12);
    let mut head = base.clone();
    head["index"]["7"]["inner"]["use"]["id"] = json!(12);
    head["index"]["11"]["inner"]["use"]["id"] = json!(8);
    head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("2");
    head["index"]["5"]["docs"] = json!("second");
    head["index"]["15"]["inner"]["assoc_const"]["value"] = json!("1");
    head["index"]["15"]["docs"] = json!("first");
    let (before, after) = extract_pair(&base, &head, "demo");
    let (values, docs) = compare(&before, &after);
    assert_eq!(values.len(), 1);
    assert_eq!((values[0].old.as_str(), values[0].new.as_str()), ("2", "1"));
    assert_eq!(values[0].path, "<demo::S as demo::T<demo::C>>::LIMIT");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].path, values[0].path);

    base["index"]["7"]["inner"]["use"]["name"] = json!("A");
    base["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 7, 13]);
    for keep_c in [false, true] {
        let mut head = base.clone();
        head["index"]["16"] = json!({"id": 16, "crate_id": 0, "visibility": "public",
            "inner": {"use": {"id": 12, "name": "Z", "is_glob": false}}});
        head["index"]["0"]["inner"]["module"]["items"] = if keep_c {
            json!([1, 3, 16, 7, 13, 11])
        } else {
            json!([1, 3, 16, 7, 11])
        };
        head["index"]["7"]["inner"]["use"]["id"] = json!(12);
        head["index"]["11"]["inner"]["use"]["id"] = json!(8);
        let (before, after) = extract_pair(&base, &head, "demo");
        let (values, docs) = compare(&before, &after);
        assert_eq!(values.len(), 1);
        assert_eq!((values[0].old.as_str(), values[0].new.as_str()), ("1", "2"));
        assert_eq!(values[0].path, "<demo::S as demo::T<demo::A>>::LIMIT");
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].path, values[0].path);
    }
}

#[test]
fn shared_argument_aliases_do_not_equate_repeated_types_with_distinct_types() {
    let mut base = nested_impl_fixture(true);
    base["index"]["0"]["inner"]["module"]["items"] = json!([1, 3, 7, 11]);
    base["index"]["7"]["inner"]["use"]["name"] = json!("A");
    base["index"]["11"]["inner"]["use"]["name"] = json!("B");
    let mut head = base.clone();
    head["index"]["12"] = json!({"id": 12, "crate_id": 0, "name": "Other",
        "visibility": "public", "inner": {"struct": {"kind": "unit", "impls": []}}});
    head["paths"]["12"] = json!({"crate_id": 0, "path": ["demo", "moved", "Other"]});
    head["index"]["7"]["inner"]["use"]["id"] = json!(12);
    head["index"]["2"]["inner"]["impl"]["trait"]["args"]["angle_bracketed"]["args"][0] =
        json!({"type": {"resolved_path": {"id": 12, "path": "Other", "args": null}}});
    head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
    head["index"]["5"]["docs"] = json!("different impl");
    let (before, after) = extract_pair(&base, &head, "demo");
    let (values, docs) = compare(&before, &after);
    assert!(values.is_empty());
    assert!(docs.is_empty());
}

#[test]
fn foreign_owner_impl_values_and_docs_include_overrides_and_inherited_defaults() {
    for inherited in [false, true] {
        let mut base = nested_impl_fixture(false);
        base["index"]["0"]["inner"]["module"]["items"] = json!([3]);
        base["index"]["3"]["inner"]["trait"]["implementations"] = json!([2]);
        base["index"]["2"]["inner"]["impl"]["trait"]["args"] = Value::Null;
        base["paths"]["90"] = json!({"crate_id": 1, "path": ["alloc", "vec", "Vec"]});
        base["index"]["2"]["inner"]["impl"]["for"] = json!({"resolved_path": {
            "id": 90, "path": "Vec", "args": {"angle_bracketed": {
                "args": [{"type": {"primitive": "u8"}}], "constraints": []
            }}
        }});
        if inherited {
            base["index"]["2"]["inner"]["impl"]["items"] = json!([]);
            base["index"]["4"]["docs"] = json!("inherited docs");
        }
        let mut head = base.clone();
        head["index"]["2"]["inner"]["impl"]["items"] = json!([5]);
        head["index"]["5"]["inner"]["assoc_const"]["value"] = json!("3");
        head["index"]["5"]["docs"] = json!("new override");
        let paired = extract_pair(&base, &head, "demo");
        let standalone = (
            extract(&base.to_string(), "demo").unwrap(),
            extract(&head.to_string(), "demo").unwrap(),
        );
        for (before, after) in [paired, standalone] {
            let (values, docs) = compare(&before, &after);
            let path = "<alloc::vec::Vec<u8> as demo::T>::LIMIT";
            assert_eq!(values.len(), 1);
            assert_eq!(values[0].path, path);
            assert_eq!(values[0].old, if inherited { "1" } else { "2" });
            assert_eq!(values[0].new, "3");
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0].path, path);
        }
    }
}
