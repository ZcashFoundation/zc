use super::*;

#[test]
fn extracts_only_concrete_trait_impl_members_and_sorts_rows() {
    let json = r#"{
        "index": {
            "impl-z": {"inner": {"impl": {
                "trait": {"path": "core::fmt::Display"},
                "for": {"resolved_path": {"path": "demo::Widget"}},
                "items": [12, "11", "missing"]
            }}},
            "11": {"name": "fmt"},
            "12": {"name": "Output"},
            "impl-inherent": {"inner": {"impl": {
                "trait": null,
                "for": {"resolved_path": {"path": "demo::Widget"}},
                "items": ["13"]
            }}},
            "13": {"name": "new"},
            "impl-blanket": {"inner": {"impl": {
                "trait": {"path": "core::convert::Into"},
                "for": {"borrowed_ref": {"type": {"generic": "T"}}},
                "items": ["14"]
            }}},
            "14": {"name": "into"}
        }
    }"#;

    assert_eq!(
        extract(json),
        vec![
            (
                "Widget".into(),
                "Output".into(),
                "core::fmt::Display".into()
            ),
            ("Widget".into(), "fmt".into(), "core::fmt::Display".into()),
        ]
    );
}

#[test]
fn deduplicates_rows_and_keeps_first_trait_on_key_collision() {
    let rows = vec![
        ("Widget".into(), "item".into(), "a::First".into()),
        ("Widget".into(), "item".into(), "a::First".into()),
        ("Widget".into(), "item".into(), "z::Second".into()),
    ];

    let map = rows_to_map(&rows);
    assert_eq!(map.len(), 1);
    assert_eq!(
        map.get(&("Widget".into(), "item".into()))
            .map(String::as_str),
        Some("a::First")
    );
}

#[test]
fn parses_bash_compatible_three_column_tsv() {
    let map = parse_tsv("Widget\titem\ta::First\nWidget\titem\tz::Second\n");
    assert_eq!(
        map.get(&("Widget".into(), "item".into()))
            .map(String::as_str),
        Some("a::First")
    );
}
