use super::*;
use serde_json::json;

fn compare(base: &Value, head: &Value) -> Vec<String> {
    super::compare(base, head).unwrap()
}

fn fixture() -> Value {
    let mut root = json!({"root": 0, "index": {}, "paths": {}});
    let items = [
        json!({"id": 0, "name": "demo", "visibility": "public",
            "inner": {"module": {"items": [1, 4, 7]}}}),
        json!({"id": 1, "name": "State", "visibility": "public", "attrs": [],
            "inner": {"enum": {"variants": [2], "impls": []}}}),
        json!({"id": 2, "name": "Ready", "visibility": "default",
            "inner": {"variant": {"kind": "plain"}}}),
        json!({"id": 3, "name": "Pending", "visibility": "default",
            "inner": {"variant": {"kind": "plain"}}}),
        json!({"id": 4, "name": "Record", "visibility": "public", "attrs": [],
            "inner": {"struct": {"kind": {"plain": {
                "fields": [5], "has_stripped_fields": false
            }}, "impls": []}}}),
        json!({"id": 5, "name": "old", "visibility": "public", "inner": {"struct_field": {}}}),
        json!({"id": 6, "name": "new", "visibility": "public", "inner": {"struct_field": {}}}),
        json!({"id": 7, "name": "Required", "visibility": "public",
            "inner": {"trait": {"items": [8], "bounds": []}}}),
        json!({"id": 8, "name": "old", "visibility": "default",
            "inner": {"function": {"has_body": false}}}),
        json!({"id": 9, "name": "required", "visibility": "default",
            "inner": {"function": {"has_body": false}}}),
        json!({"id": 10, "name": "provided", "visibility": "default",
            "inner": {"function": {"has_body": true}}}),
    ];
    for mut item in items {
        item["crate_id"] = json!(0);
        let id = item["id"].to_string();
        root["index"][id] = item;
    }
    root
}

fn add_members(root: &mut Value) {
    root["index"]["1"]["inner"]["enum"]["variants"] = json!([2, 3]);
    root["index"]["4"]["inner"]["struct"]["kind"]["plain"]["fields"] = json!([5, 6]);
    root["index"]["7"]["inner"]["trait"]["items"] = json!([8, 9, 10]);
}

#[test]
fn detects_downstream_match_literal_and_required_method_breaks() {
    let base = fixture();
    let mut head = base.clone();
    add_members(&mut head);
    let changes = compare(&base, &head);
    for path in [
        "demo::State::Pending",
        "demo::Record::new",
        "demo::Required::required",
    ] {
        assert!(
            changes.iter().any(|change| change.starts_with(path)),
            "missing {path}"
        );
    }
    assert!(!changes.iter().any(|change| change.contains("provided")));
    assert_eq!(changes.len(), 3);
}

#[test]
fn respects_nonexhaustive_private_fields_default_methods_and_new_owners() {
    let mut base = fixture();
    base["index"]["1"]["attrs"] = json!(["non_exhaustive"]);
    base["index"]["4"]["attrs"] = json!(["#[non_exhaustive]"]);
    let mut head = base.clone();
    add_members(&mut head);
    head["index"]["9"]["inner"]["function"]["has_body"] = json!(true);
    assert!(compare(&base, &head).is_empty());

    base["index"]["4"]["attrs"] = json!([]);
    base["index"]["4"]["inner"]["struct"]["kind"]["plain"]["has_stripped_fields"] = json!(true);
    assert!(compare(&base, &head).is_empty());
    base["index"]["4"]["inner"]["struct"]["kind"]["plain"]["has_stripped_fields"] = json!(false);
    base["index"]["5"]["visibility"] = json!("default");
    assert!(compare(&base, &head).is_empty());
    base["index"]["4"]["inner"]["struct"]["kind"] = json!({"tuple": [5, null]});
    assert!(compare(&base, &head).is_empty());

    let mut empty = fixture();
    empty["index"]["0"]["inner"]["module"]["items"] = json!([]);
    assert!(compare(&empty, &head).is_empty());
}

#[test]
fn losing_literal_construction_without_adding_exported_fields_is_breaking() {
    let base = fixture();
    let mut head = base.clone();
    head["index"]["4"]["inner"]["struct"]["kind"]["plain"]["has_stripped_fields"] = json!(true);
    let expected = ["demo::Record: struct is no longer literal-constructible"];
    assert_eq!(compare(&base, &head), expected);

    head["index"]["4"]["inner"]["struct"]["kind"]["plain"]["has_stripped_fields"] = json!(false);
    head["index"]["4"]["inner"]["struct"]["kind"]["plain"]["fields"] = json!([5, 6]);
    head["index"]["6"]["visibility"] = json!("default");
    assert_eq!(compare(&base, &head), expected);

    let mut nonexhaustive = base.clone();
    nonexhaustive["index"]["4"]["attrs"] = json!(["non_exhaustive"]);
    assert!(compare(&nonexhaustive, &head).is_empty());
}

fn record_variant_fixture() -> Value {
    let mut root = fixture();
    root["index"]["0"]["inner"]["module"]["items"] = json!([1]);
    root["index"]["2"]["inner"]["variant"]["kind"] =
        json!({"struct": {"fields": [5], "has_stripped_fields": false}});
    for id in ["5", "6"] {
        root["index"][id]["visibility"] = json!("default");
        root["index"][id]["inner"]["struct_field"] = json!({"primitive": "u8"});
    }
    root
}

#[test]
fn existing_record_variant_field_addition_is_breaking_at_exported_variant_path() {
    let mut base = record_variant_fixture();
    base["index"]["30"] = json!({"id": 30, "crate_id": 0, "name": "export",
        "visibility": "public",
        "inner": {"use": {"id": 1, "name": "Exported", "is_glob": false}}});
    base["index"]["0"]["inner"]["module"]["items"] = json!([30]);
    let mut head = base.clone();
    head["index"]["2"]["inner"]["variant"]["kind"]["struct"]["fields"] = json!([5, 6]);
    let changes = compare(&base, &head);
    assert_eq!(changes.len(), 1);
    assert!(changes[0].starts_with("demo::Exported::Ready:"));
}

#[test]
fn enum_alias_variant_fields_break_construction_at_the_alias_path() {
    let mut base = record_variant_fixture();
    base["index"]["30"] = json!({"id": 30, "crate_id": 0, "name": "E",
    "visibility": "public", "inner": {"type_alias": {
        "type": {"resolved_path": {"id": 1, "path": "Hidden", "args": null}},
        "generics": {"params": [], "where_predicates": []}
    }}});
    base["index"]["0"]["inner"]["module"]["items"] = json!([30]);
    let mut head = base.clone();
    head["index"]["2"]["inner"]["variant"]["kind"]["struct"]["fields"] = json!([5, 6]);
    let changes = compare(&base, &head);
    assert_eq!(changes.len(), 1);
    assert!(changes[0].starts_with("demo::E::Ready:"));
    base["index"]["2"]["attrs"] = json!(["#[non_exhaustive]"]);
    head["index"]["2"]["attrs"] = base["index"]["2"]["attrs"].clone();
    assert!(compare(&base, &head).is_empty());
}

#[test]
fn only_variant_nonexhaustiveness_allows_adding_fields_to_existing_variants() {
    for tuple in [false, true] {
        for enum_nonexhaustive in [false, true] {
            for variant_nonexhaustive in [false, true] {
                let mut base = record_variant_fixture();
                if tuple {
                    base["index"]["2"]["inner"]["variant"]["kind"] = json!({"tuple": [5]});
                    base["index"]["5"]["name"] = json!("0");
                    base["index"]["6"]["name"] = json!("1");
                }
                if enum_nonexhaustive {
                    base["index"]["1"]["attrs"] = json!(["non_exhaustive"]);
                }
                if variant_nonexhaustive {
                    base["index"]["2"]["attrs"] = json!(["#[non_exhaustive]"]);
                }
                let mut head = base.clone();
                if tuple {
                    head["index"]["2"]["inner"]["variant"]["kind"]["tuple"] = json!([5, 6]);
                } else {
                    head["index"]["2"]["inner"]["variant"]["kind"]["struct"]["fields"] =
                        json!([5, 6]);
                }
                let changes = compare(&base, &head);
                assert_eq!(changes.len(), usize::from(!variant_nonexhaustive));
                assert!(changes
                    .iter()
                    .all(|change| change.starts_with("demo::State::Ready:")));
            }
        }
    }
}

#[test]
fn new_record_variants_and_enums_do_not_invent_existing_variant_findings() {
    let mut head = record_variant_fixture();
    head["index"]["2"]["inner"]["variant"]["kind"]["struct"]["fields"] = json!([5, 6]);
    let mut base = head.clone();
    base["index"]["1"]["inner"]["enum"]["variants"] = json!([]);
    let changes = compare(&base, &head);
    assert_eq!(changes.len(), 1);
    assert!(changes[0].starts_with("demo::State::Ready:"));

    base["index"]["0"]["inner"]["module"]["items"] = json!([]);
    assert!(compare(&base, &head).is_empty());
}

#[test]
fn local_alias_chains_dispatch_compatibility_at_exported_paths() {
    let mut base = fixture();
    for (id, target, name) in [
        (30, 1, "AliasState"),
        (32, 4, "AliasRecord"),
        (34, 7, "AliasRequired"),
    ] {
        base["index"][id.to_string()] = json!({"id": id, "crate_id": 0,
        "name": name, "visibility": "public", "inner": {"type_alias": {
            "type": {"resolved_path": {"id": id + 1, "path": "Intermediate", "args": null}},
            "generics": {"params": [], "where_predicates": []}
        }}});
        base["index"][(id + 1).to_string()] = json!({"id": id + 1, "crate_id": 0,
        "name": "Intermediate", "inner": {"type_alias": {
            "type": {"resolved_path": {"id": target, "path": "Hidden", "args": null}}
        }}});
    }
    base["index"]["0"]["inner"]["module"]["items"] = json!([30, 32, 34]);
    let mut head = base.clone();
    add_members(&mut head);
    assert_eq!(
        compare(&base, &head),
        [
            "demo::AliasRecord::new: public field added to constructible struct",
            "demo::AliasRequired::required: required trait method added",
            "demo::AliasState::Pending: variant added to exhaustive enum",
        ]
    );
}

#[test]
fn alias_dispatch_does_not_guess_cycles_foreign_or_generic_targets() {
    let mut base = fixture();
    base["index"]["30"] = json!({"id": 30, "crate_id": 0, "name": "Alias",
    "visibility": "public", "inner": {"type_alias": {
        "type": {"resolved_path": {"id": 1, "path": "State", "args": null}},
        "generics": {"params": [], "where_predicates": []}
    }}});
    base["index"]["0"]["inner"]["module"]["items"] = json!([30]);
    for generic in [false, true] {
        let mut generic_base = base.clone();
        if generic {
            generic_base["index"]["30"]["inner"]["type_alias"]["generics"]["params"] =
                json!([{"name": "T", "kind": {"type": {"bounds": []}}}]);
        } else {
            generic_base["index"]["30"]["inner"]["type_alias"]["type"]["resolved_path"]["args"] = json!({"angle_bracketed": {"args": [{"type": {"primitive": "u8"}}],
                    "constraints": []}});
        }
        let mut head = generic_base.clone();
        add_members(&mut head);
        assert!(compare(&generic_base, &head).is_empty());
    }
    let mut foreign_base = base.clone();
    foreign_base["index"]["1"]["crate_id"] = json!(1);
    let mut head = foreign_base.clone();
    add_members(&mut head);
    assert!(compare(&foreign_base, &head).is_empty());

    base["index"]["30"]["inner"]["type_alias"]["type"]["resolved_path"]["id"] = json!(31);
    base["index"]["31"] = base["index"]["30"].clone();
    base["index"]["31"]["id"] = json!(31);
    base["index"]["31"]["inner"]["type_alias"]["type"]["resolved_path"]["id"] = json!(30);
    assert!(compare(&base, &base).is_empty());
}

fn trait_bound(id: u64) -> Value {
    json!([{"trait_bound": {"trait": {"id": id, "path": "Sealed", "args": null}}}])
}

#[test]
fn private_and_transitively_sealed_supertraits_are_not_downstream_implementable() {
    let mut base = fixture();
    base["index"]["20"] = json!({"id": 20, "crate_id": 0, "name": "Sealed",
        "visibility": "public", "inner": {"trait": {"items": [], "bounds": []}}});
    base["index"]["7"]["inner"]["trait"]["bounds"] = trait_bound(20);
    let mut head = base.clone();
    head["index"]["7"]["inner"]["trait"]["items"] = json!([8, 9]);
    assert!(compare(&base, &head).is_empty());

    base["index"]["21"] = base["index"]["20"].clone();
    base["index"]["21"]["id"] = json!(21);
    base["index"]["20"]["inner"]["trait"]["bounds"] = trait_bound(21);
    base["index"]["0"]["inner"]["module"]["items"] = json!([1, 4, 7, 20]);
    assert!(compare(&base, &head).is_empty());
}

#[test]
fn inaccessible_required_method_token_seals_trait_but_universal_blanket_does_not() {
    let mut base = fixture();
    base["index"]["20"] = json!({"id": 20, "crate_id": 0, "name": "Token",
        "visibility": "public", "inner": {"struct": {"kind": "unit", "impls": []}}});
    base["index"]["8"]["inner"]["function"]["sig"] = json!({"inputs": [["token",
        {"resolved_path": {"id": 20, "path": "Token", "args": null}}]]});
    let mut head = base.clone();
    head["index"]["7"]["inner"]["trait"]["items"] = json!([8, 9]);
    assert!(compare(&base, &head).is_empty());

    let mut aliased = base.clone();
    aliased["index"]["22"] = json!({"id": 22, "crate_id": 0, "name": "PublicToken",
    "visibility": "public", "inner": {"type_alias": {"type": {"resolved_path": {
        "id": 20, "path": "Token", "args": null
    }}}}});
    aliased["index"]["0"]["inner"]["module"]["items"] = json!([1, 4, 7, 22]);
    let mut alias_head = aliased.clone();
    alias_head["index"]["7"]["inner"]["trait"]["items"] = json!([8, 9]);
    assert!(compare(&aliased, &alias_head)
        .iter()
        .any(|change| change.contains("Required::required")));

    base["index"]["8"]["inner"]["function"]["sig"] = json!({});
    base["index"]["20"]["inner"] = json!({"trait": {
        "items": [], "bounds": [], "implementations": [21]
    }});
    base["index"]["21"] = json!({"id": 21, "crate_id": 0, "inner": {"impl": {
        "for": {"generic": "T"}, "generics": {"where_predicates": [], "params": [
            {"name": "T", "kind": {"type": {"bounds": []}}}
        ]}
    }}});
    base["index"]["7"]["inner"]["trait"]["bounds"] = trait_bound(20);
    assert!(compare(&base, &head)
        .iter()
        .any(|change| change.contains("Required::required")));
}

#[test]
fn relaxed_sized_and_publicly_satisfiable_blanket_bounds_do_not_seal() {
    let mut base = fixture();
    base["index"]["20"] = json!({"id": 20, "crate_id": 0, "name": "Sealed",
    "visibility": "public", "inner": {"trait": {
        "items": [], "bounds": [], "implementations": [21]
    }}});
    base["index"]["21"] = json!({"id": 21, "crate_id": 0, "inner": {"impl": {
        "for": {"generic": "T"}, "is_negative": false,
        "generics": {"where_predicates": [], "params": [
            {"name": "T", "kind": {"type": {"bounds": [{
                "trait_bound": {"trait": {"id": 99, "path": "Sized", "args": null},
                    "modifier": "maybe", "generic_params": []}
            }]}}}
        ]}
    }}});
    base["paths"]["99"] = json!({"crate_id": 1, "path": ["core", "marker", "Sized"]});
    base["index"]["7"]["inner"]["trait"]["bounds"] = trait_bound(20);
    let mut head = base.clone();
    head["index"]["7"]["inner"]["trait"]["items"] = json!([8, 9]);
    assert_eq!(
        compare(&base, &head),
        ["demo::Required::required: required trait method added"]
    );

    base["index"]["21"]["inner"]["impl"]["generics"]["params"][0]["kind"]["type"]["bounds"]
        .as_array_mut()
        .unwrap()
        .push(json!({"trait_bound": {
            "trait": {"id": 98, "path": "Copy", "args": null},
            "modifier": "none", "generic_params": []
        }}));
    base["paths"]["98"] = json!({"crate_id": 1, "path": ["core", "marker", "Copy"]});
    assert_eq!(
        compare(&base, &head),
        ["demo::Required::required: required trait method added"]
    );

    base["index"]["21"]["inner"]["impl"]["generics"]["params"][0]["kind"]["type"]["bounds"] =
        json!([]);
    base["index"]["21"]["inner"]["impl"]["generics"]["where_predicates"] = json!([{
        "bound_predicate": {"type": {"generic": "T"}, "generic_params": [],
            "bounds": [{"trait_bound": {
                "trait": {"id": 99, "path": "Sized", "args": null},
                "modifier": "maybe", "generic_params": []
            }}]}
    }]);
    assert_eq!(
        compare(&base, &head),
        ["demo::Required::required: required trait method added"]
    );

    // A reachable local bound can be implemented by the consumer as well.
    base["index"]["22"] = json!({"id": 22, "crate_id": 0, "name": "PublicBound",
        "visibility": "public", "inner": {"trait": {"items": [], "bounds": []}}});
    base["index"]["0"]["inner"]["module"]["items"] = json!([1, 4, 7, 22]);
    base["index"]["21"]["inner"]["impl"]["generics"]["where_predicates"][0]["bound_predicate"]
        ["bounds"] = trait_bound(22);
    assert_eq!(
        compare(&base, &head),
        ["demo::Required::required: required trait method added"]
    );

    // Removing the only exported path makes this bound genuinely private.
    base["index"]["0"]["inner"]["module"]["items"] = json!([1, 4, 7]);
    assert!(compare(&base, &head).is_empty());

    base["index"]["21"]["inner"]["impl"]["generics"]["params"][0]["kind"]["type"]["bounds"] =
        json!([]);
    base["index"]["21"]["inner"]["impl"]["generics"]["where_predicates"] =
        json!([{"bound_predicate": {"type": {"generic": "T"}, "bounds": trait_bound(20)}}]);
    assert!(compare(&base, &head).is_empty());
}

#[test]
fn read_and_parse_failures_are_not_clean_compatibility_results() {
    let path = std::env::temp_dir().join(format!("zc-compat-invalid-{}", std::process::id()));
    fs::write(&path, "{}").unwrap();
    assert!(breaking_additions(&path, &path).is_err());
    fs::remove_file(&path).unwrap();
    assert!(breaking_additions(&path, &path).is_err());
}

#[test]
fn unsupported_blanket_constraints_are_analysis_errors_not_sealing_evidence() {
    let mut base = fixture();
    base["index"]["20"] = json!({"id": 20, "crate_id": 0, "name": "Sealed",
    "visibility": "public", "inner": {"trait": {
        "items": [], "bounds": [], "implementations": [21]
    }}});
    base["index"]["21"] = json!({"id": 21, "crate_id": 0, "inner": {"impl": {
        "for": {"generic": "T"}, "generics": {
            "params": [{"name": "T", "kind": {"type": {"bounds": []}}}],
            "where_predicates": [{"eq_predicate": {
                "lhs": {"generic": "T"}, "rhs": {"primitive": "u8"}
            }}]
        }
    }}});
    base["index"]["7"]["inner"]["trait"]["bounds"] = trait_bound(20);
    assert!(super::compare(&base, &base).unwrap().is_empty());
    let mut head = base.clone();
    head["index"]["7"]["inner"]["trait"]["items"] = json!([8, 9]);
    assert!(super::compare(&base, &head).is_err());

    base["index"]["21"]["inner"]["impl"]["generics"]["where_predicates"] = json!([]);
    base["index"]["21"]["inner"]["impl"]["generics"]["params"][0]["kind"]["type"]["bounds"] = json!([{"trait_bound": {
        "trait": {"id": 99, "path": "Iterator", "args": {
            "angle_bracketed": {"args": [], "constraints": [{
                "name": "Item", "binding": {"equality": {"type": {"primitive": "u8"}}}
            }]}
        }},
        "modifier": "none", "generic_params": []
    }}]);
    assert!(super::compare(&base, &head).is_err());

    base["index"]["21"]["inner"]["impl"]["generics"]["params"][0]["kind"]["type"]["bounds"] =
        json!([]);
    base["index"]["21"]["inner"]["impl"]["for"] =
        json!({"borrowed_ref": {"lifetime": null, "is_mutable": false, "type": {"generic": "T"}}});
    assert!(super::compare(&base, &head).is_err());
}

#[test]
fn consumer_boundary_distinguishes_satisfiable_blankets_from_private_sealing() {
    use std::process::Command;

    let dir = std::env::temp_dir().join(format!("zc-compat-consumer-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let library = dir.join("boundary.rs");
    let artifact = dir.join("libboundary.rlib");
    let consumer = dir.join("consumer.rs");
    let compile_library = |source: &str| {
        fs::write(&library, source).unwrap();
        let output = Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-name",
                "boundary",
                "--crate-type=rlib",
            ])
            .arg(&library)
            .arg("-o")
            .arg(&artifact)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let compile_consumer = |source: &str| {
        fs::write(&consumer, source).unwrap();
        Command::new("rustc")
            .args(["--edition=2021", "--crate-type=lib", "--extern"])
            .arg(format!("boundary={}", artifact.display()))
            .arg(&consumer)
            .arg("-o")
            .arg(dir.join("consumer.rlib"))
            .output()
            .unwrap()
            .status
            .success()
    };
    for (blanket, local, implementable) in [
        (
            "impl<T: Copy> Sealed for T {}",
            "#[derive(Clone, Copy)] struct Local;",
            true,
        ),
        (
            "impl<T> Sealed for T where T: ?Sized {}",
            "struct Local([u8]);",
            true,
        ),
        ("", "struct Local;", false),
    ] {
        let source = format!(
            "mod hidden {{ pub trait Sealed {{}} {blanket} }} pub trait Api: hidden::Sealed {{}}"
        );
        let downstream = format!("{local} impl boundary::Api for Local {{}}");
        compile_library(&source);
        assert_eq!(compile_consumer(&downstream), implementable);

        compile_library(&format!(
            "mod hidden {{ pub trait Sealed {{}} {blanket} }} \
             pub trait Api: hidden::Sealed {{ fn required(&self); }}"
        ));
        assert!(!compile_consumer(&downstream));
        assert_eq!(
            compile_consumer(&format!(
                "{local} impl boundary::Api for Local {{ fn required(&self) {{}} }}"
            )),
            implementable
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}
