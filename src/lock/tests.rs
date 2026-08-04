use std::collections::HashSet;

use super::{attribution, build_diff, extract_lock};

#[test]
fn parser_emits_a_trailing_package_without_a_blank_line() {
    let lock = concat!(
        "version = 3\n\n",
        "[[package]]\n",
        "name = \"alpha\"\n",
        "version = \"1.0.0\"\n\n",
        "[[package]]\n",
        "name = \"omega\"\n",
        "version = \"9.0.0\"",
    );

    assert_eq!(
        extract_lock(lock),
        vec![
            ("alpha".to_string(), "1.0.0".to_string()),
            ("omega".to_string(), "9.0.0".to_string()),
        ]
    );
}

#[test]
fn attribution_deduplicates_in_first_seen_order_and_truncates() {
    let direct = HashSet::from([
        "first".to_string(),
        "second".to_string(),
        "third".to_string(),
        "fourth".to_string(),
    ]);
    let tree = concat!(
        "0root-a v1\n",
        "1first v1\n",
        "2target v1\n",
        "0root-b v1\n",
        "1second v1\n",
        "2target v1\n",
        "0root-c v1\n",
        "1first v1\n",
        "2target v1\n",
        "1third v1\n",
        "2target v1\n",
        "0root-d v1\n",
        "1fourth v1\n",
        "2target v1\n",
    );

    let via = attribution(tree, &direct);

    assert_eq!(via["target"], "first,second,third,...(+1)");
}

#[test]
fn lock_diff_groups_versions_and_suppresses_direct_dependencies() {
    let base = vec![
        ("direct".to_string(), "1.0.0".to_string()),
        ("multi".to_string(), "1.0.0".to_string()),
        ("multi".to_string(), "2.0.0".to_string()),
        ("removed".to_string(), "1.0.0".to_string()),
    ];
    let head = vec![
        ("added".to_string(), "1.0.0".to_string()),
        ("direct".to_string(), "2.0.0".to_string()),
        ("multi".to_string(), "2.0.0".to_string()),
        ("multi".to_string(), "3.0.0".to_string()),
    ];
    let direct = HashSet::from(["direct".to_string()]);

    let result = build_diff(&base, &head, &direct, "");

    assert_eq!(
        result.changed,
        vec![(
            "multi".to_string(),
            "1.0.0,2.0.0".to_string(),
            "2.0.0,3.0.0".to_string(),
        )]
    );
    assert_eq!(
        result.removed,
        vec![("removed".to_string(), "1.0.0".to_string())]
    );
    assert_eq!(
        result.added,
        vec![("added".to_string(), "1.0.0".to_string())]
    );
}
