use super::*;

#[test]
fn extracts_public_values_and_docs_in_path_order() {
    let json = r#"{
        "paths": {
            "9": {"path": ["demo", "SECOND"]},
            "2": {"path": ["demo", "FIRST"]},
            "7": {"path": ["demo", "PRIVATE"]}
        },
        "index": {
            "2": {
                "visibility": "public",
                "docs": "first docs",
                "inner": {"constant": {
                    "type": {"primitive": "u64"},
                    "const": {"value": "2", "expr": "1 + 1"}
                }}
            },
            "7": {
                "visibility": "crate",
                "docs": "hidden",
                "inner": {"constant": {
                    "type": {"primitive": "u8"}, "const": {"value": "1"}
                }}
            },
            "9": {
                "visibility": "public",
                "docs": "",
                "inner": {"static": {
                    "type": {"resolved_path": {"name": "Widget"}}, "expr": "make()"
                }}
            }
        }
    }"#;

    let rows = extract(json, "demo-crate");
    assert_eq!(rows.len(), 3);
    match &rows[0] {
        IndexRow::Value(row) => {
            assert_eq!(row.path, "demo::SECOND");
            assert_eq!(row.ty, "Widget");
            assert_eq!(row.value, "make()");
        }
        IndexRow::Doc(_) => panic!("the first path should produce a value row"),
    }
    match &rows[1] {
        IndexRow::Value(row) => {
            assert_eq!(row.path, "demo::FIRST");
            assert_eq!(row.ty, "u64");
            assert_eq!(row.value, "2");
        }
        IndexRow::Doc(_) => panic!("the second path should produce a value row first"),
    }
    match &rows[2] {
        IndexRow::Doc(row) => {
            assert_eq!(row.path, "demo::FIRST");
            assert_eq!(row.docs, "Zmlyc3QgZG9jcw==");
        }
        IndexRow::Value(_) => panic!("the item should produce its documentation row second"),
    }
}

#[test]
fn compares_only_shared_items_in_head_order_and_uses_base_type() {
    let base = vec![
        IndexRow::Value(ValueRow {
            crate_name: "demo".into(),
            path: "demo::A".into(),
            ty: "OldA".into(),
            value: "1".into(),
        }),
        IndexRow::Value(ValueRow {
            crate_name: "demo".into(),
            path: "demo::B".into(),
            ty: "OldB".into(),
            value: "2".into(),
        }),
        IndexRow::Doc(DocRow {
            crate_name: "demo".into(),
            path: "demo::A".into(),
            docs: "old".into(),
        }),
    ];
    let head = vec![
        IndexRow::Value(ValueRow {
            crate_name: "demo".into(),
            path: "demo::B".into(),
            ty: "NewB".into(),
            value: "20".into(),
        }),
        IndexRow::Value(ValueRow {
            crate_name: "demo".into(),
            path: "demo::ADDED".into(),
            ty: "Added".into(),
            value: "3".into(),
        }),
        IndexRow::Value(ValueRow {
            crate_name: "demo".into(),
            path: "demo::A".into(),
            ty: "NewA".into(),
            value: "10".into(),
        }),
        IndexRow::Doc(DocRow {
            crate_name: "demo".into(),
            path: "demo::A".into(),
            docs: "new".into(),
        }),
        IndexRow::Doc(DocRow {
            crate_name: "demo".into(),
            path: "demo::ADDED".into(),
            docs: "new item".into(),
        }),
    ];

    let (values, docs) = compare(&base, &head);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].path, "demo::B");
    assert_eq!(values[0].ty, "OldB");
    assert_eq!(values[0].old, "2");
    assert_eq!(values[0].new, "20");
    assert_eq!(values[1].path, "demo::A");
    assert_eq!(values[1].ty, "OldA");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].path, "demo::A");
}
