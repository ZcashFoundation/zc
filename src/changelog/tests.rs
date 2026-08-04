use super::render;
use crate::model::Section;
use crate::traitmap::TraitMap;

struct Case {
    name: &'static str,
    lines: &'static [&'static str],
    section: Section,
    traits: &'static [((&'static str, &'static str), &'static str)],
    expected: &'static [&'static str],
}

#[test]
fn renders_librustzcash_changelog_groups() {
    let cases = [
        Case {
            name: "brace-grouped members",
            lines: &[
                "pub struct fixture::Widget",
                "impl fixture::Widget",
                "pub fn fixture::Widget::new() -> fixture::Widget",
                "pub fn fixture::Widget::run(&self)",
            ],
            section: Section::Added,
            traits: &[],
            expected: &["- `Widget::{new, run}`"],
        },
        Case {
            name: "over-wide members",
            lines: &[
                "pub struct fixture::Verifier",
                "impl fixture::Verifier",
                "pub fn fixture::Verifier::check_cross_address_disabled(&self)",
                "pub fn fixture::Verifier::enforce_nullifier_uniqueness(&self)",
                "pub fn fixture::Verifier::validate_ironwood_proof_size(&self)",
                "pub fn fixture::Verifier::validate_orchard_value_balance(&self)",
            ],
            section: Section::Added,
            traits: &[],
            expected: &[
                "- `Verifier`:",
                "  - `check_cross_address_disabled`",
                "  - `enforce_nullifier_uniqueness`",
                "  - `validate_ironwood_proof_size`",
                "  - `validate_orchard_value_balance`",
            ],
        },
        Case {
            name: "associated item with Self generics",
            lines: &["pub type fixture::Foo<u32>::Bytes = [u8; 48]"],
            section: Section::Added,
            traits: &[(("Foo", "Bytes"), "IntoDisk")],
            expected: &["- `impl IntoDisk for Foo<u32>`:", "  - `Bytes`"],
        },
        Case {
            name: "changed signature pair",
            lines: &[
                "  - pub fn fixture::f() -> u8",
                "  + pub fn fixture::f() -> u16",
            ],
            section: Section::Changed,
            traits: &[],
            expected: &["- `fn f() -> u8`", "  → `fn f() -> u16`"],
        },
        Case {
            name: "marker over several types",
            lines: &[
                "impl fixture::Marker for fixture::A",
                "impl fixture::Marker for fixture::B",
            ],
            section: Section::Added,
            traits: &[],
            expected: &["- `impl Marker` for:", "  - `A`", "  - `B`"],
        },
        Case {
            name: "impl lifetime stripped",
            lines: &[
                "impl<'a> core::convert::From<&'a u8> for fixture::Foo",
                "pub fn fixture::Foo::from(_: &'a u8) -> fixture::Foo",
            ],
            section: Section::Added,
            traits: &[(("Foo", "from"), "From")],
            expected: &["- `impl From<&u8> for Foo`"],
        },
        Case {
            name: "nested generic paths shortened",
            lines: &[
                "impl core::convert::From<core::option::Option<fixture::sub::Bar>> for fixture::Foo",
                "pub fn fixture::Foo::from(_: core::option::Option<fixture::sub::Bar>) -> fixture::Foo",
            ],
            section: Section::Added,
            traits: &[(("Foo", "from"), "From")],
            expected: &["- `impl From<option::Option<sub::Bar>> for Foo`"],
        },
        Case {
            name: "derives collapse by Self type",
            lines: &[
                "#[derive(Clone, Debug)] pub struct fixture::Bar",
                "impl core::clone::Clone for fixture::Bar",
                "impl core::fmt::Debug for fixture::Bar",
            ],
            section: Section::Added,
            traits: &[],
            expected: &["- `Bar`", "- `impl {Clone, Debug} for Bar`"],
        },
        Case {
            name: "whole module subsumes contents",
            lines: &[
                "pub mod fixture::m",
                "pub struct fixture::m::Foo",
                "pub fn fixture::m::g()",
                "pub fn fixture::sibling() -> u8",
            ],
            section: Section::Added,
            traits: &[],
            expected: &["- `m`", "- `sibling`"],
        },
    ];

    for case in cases {
        let lines = case
            .lines
            .iter()
            .map(|line| (*line).to_string())
            .collect::<Vec<_>>();
        let mut traits = TraitMap::new();
        for &((self_name, member), trait_path) in case.traits {
            traits.insert(
                (self_name.to_string(), member.to_string()),
                trait_path.to_string(),
            );
        }
        let expected = case
            .expected
            .iter()
            .map(|line| (*line).to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            render(&lines, case.section, "fixture", &traits),
            expected,
            "{}",
            case.name
        );
    }
}
