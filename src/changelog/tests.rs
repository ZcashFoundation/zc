use super::render;
use crate::model::Section;
use crate::traitmap::TraitMap;

fn rendered(lines: &[&str], section: Section, traits: &TraitMap) -> String {
    let lines = lines
        .iter()
        .map(|line| (*line).to_string())
        .collect::<Vec<_>>();
    render(&lines, section, "fixture", traits).join("\n")
}

#[test]
fn ordinary_proptest_names_are_public_apis() {
    let traits = TraitMap::from([(
        ("fixture::Generated".into(), "Strategy".into()),
        Some("proptest::arbitrary::Arbitrary".into()),
    )]);
    let added = rendered(
        &[
            "pub type fixture::Strategy = u8",
            "pub type fixture::Parameters = ()",
            "pub fn fixture::arbitrary()",
            "pub fn fixture::arbitrary_with(usize)",
            "pub fn fixture::uses_strategy(proptest::strategy::BoxedStrategy<u8>)",
            "impl fixture::Arbitrary for fixture::Ordinary",
            "impl proptest::arbitrary::Arbitrary for fixture::Generated",
            "pub type fixture::Generated::Strategy = u8",
        ],
        Section::Added,
        &traits,
    );
    for name in [
        "Strategy",
        "Parameters",
        "arbitrary",
        "arbitrary_with",
        "uses_strategy",
        "Arbitrary for Ordinary",
    ] {
        assert!(added.contains(name), "lost {name}: {added}");
    }
    assert!(!added.contains("Generated"));
    let changed = rendered(
        &[
            "  - pub type fixture::Strategy = u8",
            "  + pub type fixture::Strategy = u16",
            "  - pub type fixture::Parameters = u8",
            "  + pub type fixture::Parameters = u16",
        ],
        Section::Changed,
        &traits,
    );
    for signature in [
        "Strategy = u8",
        "Strategy = u16",
        "Parameters = u8",
        "Parameters = u16",
    ] {
        assert!(changed.contains(signature), "lost {signature}: {changed}");
    }
}

#[test]
fn trait_arguments_and_full_owner_paths_remain_distinct() {
    let result = rendered(
        &[
            "impl core::convert::From<u8> for fixture::left::S",
            "pub fn fixture::left::S::from(u8) -> Self",
            "impl core::convert::From<u16> for fixture::left::S",
            "pub fn fixture::left::S::from(u16) -> Self",
            "impl fixture::left::Trait for fixture::right::S",
            "impl fixture::right::Trait for fixture::right::S",
        ],
        Section::Added,
        &TraitMap::new(),
    );
    for identity in [
        "From<u8>",
        "From<u16>",
        "left::S",
        "right::S",
        "left::Trait",
        "right::Trait",
    ] {
        assert!(result.contains(identity), "lost {identity}: {result}");
    }
}

#[test]
fn inherent_or_ambiguous_methods_do_not_invent_trait_impls() {
    let traits = TraitMap::from([
        (("fixture::S".into(), "default".into()), None),
        (
            ("fixture::left::S".into(), "item".into()),
            Some("fixture::Trait<u8>".into()),
        ),
        (("fixture::right::S".into(), "item".into()), None),
    ]);
    let result = rendered(
        &[
            "impl fixture::S",
            "pub fn fixture::S::default(usize) -> Self",
            "pub fn fixture::left::S::item(&self)",
            "pub fn fixture::right::S::item(&self)",
        ],
        Section::Added,
        &traits,
    );
    assert!(result.contains("S::default"));
    assert!(!result.contains("impl Default"));
    assert!(result.contains("Trait<u8> for left::S"));
    assert!(result.contains("right::S::item"));
    assert!(!result.contains("Trait<u8> for right::S"));
}

#[test]
fn changed_signatures_keep_all_semantic_attributes() {
    let result = rendered(
        &[
            "  - #[repr(C)] pub struct fixture::S",
            "  + #[repr(C)] #[non_exhaustive] pub struct fixture::S",
            "  - #[no_mangle] pub c fn fixture::exported()",
            "  + #[link_section = \".custom\"] #[no_mangle] pub c fn fixture::exported()",
        ],
        Section::Changed,
        &TraitMap::new(),
    );
    assert!(result.contains("#[repr(C)] struct S"));
    assert!(result.contains("#[repr(C)] #[non_exhaustive] struct S"));
    assert!(result.contains("#[link_section = \".custom\"] #[no_mangle] c fn exported()"));
}

#[test]
fn pinned_macro_proc_macro_and_abi_declarations_keep_their_names() {
    let result = rendered(
        &[
            "pub macro fixture::one!",
            "pub macro fixture::two!",
            "pub proc macro fixture::#[derive(First)]",
            "pub proc macro fixture::#[derive(Second)]",
            "pub proc macro fixture::function_like!()",
            "pub proc macro fixture::#[attribute_like]",
            "#[link_section = \".custom\"] #[no_mangle] pub static fixture::COUNT: u8",
            "pub c fn fixture::exported()",
            "pub unsafe extern \"C-unwind\" fn fixture::unwindable()",
        ],
        Section::Added,
        &TraitMap::new(),
    );
    for name in [
        "one!",
        "two!",
        "First",
        "Second",
        "function_like!",
        "attribute_like",
        "COUNT",
        "exported",
        "unwindable",
    ] {
        assert!(result.contains(name), "lost {name}: {result}");
    }
}

#[test]
fn concise_member_groups_keep_existing_braces() {
    let result = rendered(
        &[
            "pub struct fixture::Widget",
            "impl fixture::Widget",
            "pub fn fixture::Widget::new() -> Self",
            "pub fn fixture::Widget::run(&self)",
        ],
        Section::Added,
        &TraitMap::new(),
    );
    assert_eq!(result, "- `Widget::{new, run}`");
}

#[test]
fn generic_trait_impls_keep_lifetimes_and_owner_arguments() {
    let traits = TraitMap::from([(
        ("fixture::S<T>".into(), "from".into()),
        Some("core::convert::From<&'a T>".into()),
    )]);
    let header = "impl<'a, T> core::convert::From<&'a T> for fixture::S<T>";
    let member = "pub fn fixture::S<T>::from(&'a T) -> Self";
    for lines in [[header, member], [member, header]] {
        let result = rendered(
            &[
                lines[0],
                lines[1],
                "impl fixture::Trait<u16> for fixture::S<u16>",
            ],
            Section::Added,
            &traits,
        );
        assert!(
            result.contains("impl<'a, T> From<&'a T> for S<T>"),
            "{result}"
        );
        assert!(result.contains("Trait<u16> for S<u16>"));
    }
}

#[test]
fn bounded_impl_headers_and_attributed_members_share_one_group_in_either_order() {
    let traits = TraitMap::from([(
        ("fixture::S<T>".into(), "go".into()),
        Some("fixture::Public".into()),
    )]);
    let header = "impl<T> fixture::Public for fixture::S<T> where T: core::marker::Copy";
    let member = "#[must_use] pub fn fixture::S<T>::go(&self) -> T";
    for lines in [[header, member], [member, header]] {
        assert_eq!(
            rendered(&lines, Section::Added, &traits),
            "- `impl<T> Public for S<T> where T: core::marker::Copy`:\n  - `go`",
        );
    }
}

#[test]
fn where_tokens_inside_const_arguments_do_not_split_impl_identity() {
    let owner = "fixture::S<{ \" where \".len() }>";
    let traits = TraitMap::from([((owner.into(), "go".into()), Some("fixture::Public".into()))]);
    let header =
        "impl fixture::Public for fixture::S<{ \" where \".len() }> where Self: core::marker::Copy";
    let member = "pub fn fixture::S<{ \" where \".len() }>::go(&self)";
    for lines in [[header, member], [member, header]] {
        assert_eq!(
            rendered(&lines, Section::Added, &traits),
            "- `impl Public for S<{ \" where \".len() }> where Self: core::marker::Copy`:\n  - `go`",
        );
    }
}

#[test]
fn generic_method_and_gat_binders_do_not_change_trait_ownership() {
    let traits = TraitMap::from([
        (
            ("fixture::S<T>".into(), "choose".into()),
            Some("fixture::Trait<T>".into()),
        ),
        (
            ("fixture::S<T>".into(), "Output".into()),
            Some("fixture::Trait<T>".into()),
        ),
    ]);
    let header = "impl<T> fixture::Trait<T> for fixture::S<T>";
    let method = "pub fn fixture::S<T>::choose<U: dependency::Bound>(&self, value: U) -> U";
    let gat = "pub type fixture::S<T>::Output<'a>: dependency::Bound";
    for lines in [[header, method, gat], [gat, method, header]] {
        let result = rendered(&lines, Section::Added, &traits);
        assert_eq!(result.matches("- `impl").count(), 1);
        assert!(result.starts_with("- `impl<T> Trait<T> for S<T>`:"));
        assert!(result.contains("`choose<U: dependency::Bound>`"));
        assert!(result.contains("`Output<'a>`"));
    }
    assert_eq!(
        rendered(&[method], Section::Added, &traits),
        "- `impl Trait<T> for S<T>`:\n  - `choose<U: dependency::Bound>`",
    );
    let ambiguous = TraitMap::from([(("fixture::S<T>".into(), "choose".into()), None)]);
    for map in [&ambiguous, &TraitMap::new()] {
        let result = rendered(&[method], Section::Added, map);
        assert!(!result.contains("impl"));
        assert!(result.contains("choose<U: dependency::Bound>"));
    }
}
