use std::collections::HashMap;

use super::group;
use crate::model::{GroupMode, GroupRecord, Section};

fn lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_string()).collect()
}

#[test]
fn groups_public_api_items() {
    struct Case {
        name: &'static str,
        lines: Vec<String>,
        section: Section,
        mode: GroupMode,
        expected: Vec<GroupRecord>,
    }

    let cases = vec![
        Case {
            name: "variant under enum",
            lines: lines(&["pub enum zc_fixture::Color", "pub zc_fixture::Color::Red"]),
            section: Section::Added,
            mode: GroupMode::Mod,
            expected: vec![
                GroupRecord::ModHeader("zc_fixture".to_string()),
                GroupRecord::TypeSub {
                    name: "Color".to_string(),
                    kind: "enum".to_string(),
                },
                GroupRecord::DeepItem("pub enum Color".to_string()),
                GroupRecord::DeepItem("pub Red".to_string()),
            ],
        },
        Case {
            name: "by type keeps type prefix",
            lines: lines(&["pub enum zc_fixture::Color", "pub zc_fixture::Color::Red"]),
            section: Section::Added,
            mode: GroupMode::Type,
            expected: vec![
                GroupRecord::TypeHeader {
                    name: "zc_fixture::Color".to_string(),
                    kind: "enum".to_string(),
                },
                GroupRecord::Item("pub enum Color".to_string()),
                GroupRecord::Item("pub Color::Red".to_string()),
            ],
        },
        Case {
            name: "generic arguments on owner",
            lines: lines(&[
                "pub struct zc_fixture::Wrap<T>",
                "pub fn zc_fixture::Wrap<u32>::get(&self)",
            ]),
            section: Section::Added,
            mode: GroupMode::Mod,
            expected: vec![
                GroupRecord::ModHeader("zc_fixture".to_string()),
                GroupRecord::TypeSub {
                    name: "Wrap".to_string(),
                    kind: "struct".to_string(),
                },
                GroupRecord::DeepItem("pub struct Wrap<T>".to_string()),
                GroupRecord::DeepItem("pub fn get(&self)".to_string()),
            ],
        },
        Case {
            name: "interleaved enum items use one header",
            lines: lines(&[
                "pub enum zc_fixture::E",
                "pub zc_fixture::E::A",
                "pub zc_fixture::E::A::x: u32",
                "pub zc_fixture::E::B",
                "pub zc_fixture::E::B::y: u32",
                "pub zc_fixture::E::Other",
            ]),
            section: Section::Added,
            mode: GroupMode::Mod,
            expected: vec![
                GroupRecord::ModHeader("zc_fixture".to_string()),
                GroupRecord::TypeSub {
                    name: "E".to_string(),
                    kind: "enum".to_string(),
                },
                GroupRecord::DeepItem("pub enum E".to_string()),
                GroupRecord::DeepItem("pub A".to_string()),
                GroupRecord::DeepItem("pub B".to_string()),
                GroupRecord::DeepItem("pub Other".to_string()),
                GroupRecord::TypeSub {
                    name: "E::A".to_string(),
                    kind: "struct".to_string(),
                },
                GroupRecord::DeepItem("pub x: u32".to_string()),
                GroupRecord::TypeSub {
                    name: "E::B".to_string(),
                    kind: "struct".to_string(),
                },
                GroupRecord::DeepItem("pub y: u32".to_string()),
            ],
        },
        Case {
            name: "foreign type follows divider",
            lines: lines(&[
                "pub fn dep::Foo::name(&self)",
                "pub fn zc_fixture::Own::go(&self)",
            ]),
            section: Section::Added,
            mode: GroupMode::Mod,
            expected: vec![
                GroupRecord::ModHeader("zc_fixture".to_string()),
                GroupRecord::TypeSub {
                    name: "Own".to_string(),
                    kind: "type".to_string(),
                },
                GroupRecord::DeepItem("pub fn go(&self)".to_string()),
                GroupRecord::ExtDivider,
                GroupRecord::ModHeader("dep".to_string()),
                GroupRecord::TypeSub {
                    name: "Foo".to_string(),
                    kind: "type".to_string(),
                },
                GroupRecord::DeepItem("pub fn name(&self)".to_string()),
            ],
        },
        Case {
            name: "changed key comes from old line",
            lines: lines(&[
                "  - pub fn zc_fixture::Thing::old(&self)",
                "  + pub fn zc_fixture::Renamed::new(&self)",
            ]),
            section: Section::Changed,
            mode: GroupMode::Mod,
            expected: vec![
                GroupRecord::ModHeader("zc_fixture".to_string()),
                GroupRecord::TypeSub {
                    name: "Thing".to_string(),
                    kind: "type".to_string(),
                },
                GroupRecord::DeepItem("  - pub fn old(&self)".to_string()),
                GroupRecord::DeepItem("  + pub fn Renamed::new(&self)".to_string()),
            ],
        },
    ];

    for case in cases {
        assert_eq!(
            group(
                &case.lines,
                case.section,
                case.mode,
                "zc_fixture",
                &HashMap::new(),
            ),
            case.expected,
            "{}",
            case.name,
        );
    }
}

#[test]
fn multiple_attributes_and_abi_functions_group_under_their_actual_module() {
    let signatures = lines(&[
        "#[link_section = \".custom\"] #[no_mangle] pub static zc_fixture::nested::COUNT: u8",
        "pub c fn zc_fixture::nested::exported()",
        "pub macro zc_fixture::nested::one!",
        "pub proc macro zc_fixture::nested::#[derive(First)]",
    ]);
    let grouped = group(
        &signatures,
        Section::Added,
        GroupMode::Mod,
        "zc_fixture",
        &HashMap::new(),
    );
    assert!(grouped.iter().any(|record| {
        matches!(record, GroupRecord::ModHeader(module) if module == "zc_fixture::nested")
    }));
    assert!(!grouped
        .iter()
        .any(|record| matches!(record, GroupRecord::ExtDivider)));
    let items = grouped
        .iter()
        .filter_map(|record| match record {
            GroupRecord::Item(item) | GroupRecord::DeepItem(item) => Some(item.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    for name in ["COUNT", "exported", "one!", "First"] {
        assert!(items.contains(name), "lost {name}: {items}");
    }
    assert!(items.contains("#[link_section = \".custom\"] #[no_mangle]"));
}

#[test]
fn identical_short_owner_names_use_independent_semantic_kinds() {
    let kinds = HashMap::from([
        ("zc_fixture::left::Same".into(), "struct".into()),
        ("zc_fixture::right::Same".into(), "trait".into()),
        ("Same".into(), "enum".into()),
    ]);
    let grouped = group(
        &lines(&[
            "pub fn zc_fixture::left::Same::item(&self)",
            "pub fn zc_fixture::right::Same::item(&self)",
        ]),
        Section::Added,
        GroupMode::Type,
        "zc_fixture",
        &kinds,
    );
    assert!(grouped.contains(&GroupRecord::TypeHeader {
        name: "zc_fixture::left::Same".into(),
        kind: "struct".into(),
    }));
    assert!(grouped.contains(&GroupRecord::TypeHeader {
        name: "zc_fixture::right::Same".into(),
        kind: "trait".into(),
    }));
}
