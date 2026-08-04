//! Librustzcash-style changelog rendering for public API diff lines.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use crate::model::Section;
use crate::traitmap::TraitMap;

const WIDTH: usize = 100;
const BOILERPLATE: &[&str] = &[
    "from",
    "into",
    "try_from",
    "try_into",
    "clone",
    "clone_from",
    "fmt",
    "hash",
    "eq",
    "ne",
    "cmp",
    "partial_cmp",
    "lt",
    "le",
    "gt",
    "ge",
    "default",
    "deref",
    "deref_mut",
    "as_ref",
    "as_mut",
    "borrow",
    "borrow_mut",
    "drop",
    "serialize",
    "deserialize",
    "into_iter",
    "next",
];

fn inner_generic_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^<>]*>").expect("valid inner-generic regex"))
}

fn long_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[A-Za-z_][A-Za-z0-9_]*::[A-Za-z_][A-Za-z0-9_]*::").expect("valid path regex")
    })
}

fn lifetime_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"'[A-Za-z_][A-Za-z0-9_]* *").expect("valid lifetime regex"))
}

fn generic_leading_comma_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"< *, *").expect("valid generic-list regex"))
}

fn generic_trailing_comma_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r" *, *>").expect("valid generic-list regex"))
}

fn attribute_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^#\[[^]]*\] +").expect("valid attribute regex"))
}
fn pub_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^pub +").expect("valid visibility regex"))
}

fn pub_module_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^pub +mod +").expect("valid public-module regex"))
}

fn impl_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^impl +").expect("valid impl-keyword regex"))
}

fn qualifiers_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:(?:const|async|unsafe) +)*").expect("valid qualifier regex"))
}

fn item_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:fn|static|type|use) +").expect("valid item-keyword regex"))
}
fn qualified_item_keyword_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:fn|static|type|use|mod|struct|enum|trait|union) +")
            .expect("valid qualified-item keyword regex")
    })
}

fn declaration_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:mod|struct|enum|trait|union) +").expect("valid declaration regex")
    })
}

fn impl_generics_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^impl *<[^<>]*>").expect("valid impl-generics regex"))
}

fn arbitrary_impl_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^impl[ <].*Arbitrary.* for ").expect("valid Arbitrary-impl regex")
    })
}

fn proptest_assoc_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:pub +)?type [^=]*::(?:Parameters|Strategy) =")
            .expect("valid proptest-associated-type regex")
    })
}

fn proptest_method_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"::(?:arbitrary|arbitrary_with)\(").expect("valid proptest-method regex")
    })
}

fn structural_eq_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^impl[ <].*Structural(?:Partial)?Eq.* for ")
            .expect("valid structural-equality regex")
    })
}

fn strip_inner_generics(mut value: String) -> String {
    while let Some(found) = inner_generic_re().find(&value) {
        value.replace_range(found.range(), "");
    }
    value
}

fn last_seg(path: &str) -> String {
    let path = strip_inner_generics(path.to_string());
    path.rsplit("::").next().unwrap_or(&path).to_string()
}

/// Returns the first balanced generic argument list, or the unmatched suffix.
fn outer_gen(path: &str) -> Option<&str> {
    let start = path.find('<')?;
    let mut depth = 0usize;
    for (offset, byte) in path.as_bytes()[start..].iter().enumerate() {
        match byte {
            b'<' => depth += 1,
            b'>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&path[start..start + offset + 1]);
                }
            }
            _ => {}
        }
    }
    Some(&path[start..])
}

fn keep2(path: &str) -> String {
    let mut path = path.to_string();
    while let Some(found) = long_path_re().find(&path) {
        let matched = &path[found.range()];
        let remove_len = matched.find("::").map_or(0, |colon| colon + 2);
        if remove_len == 0 {
            break;
        }
        path.replace_range(found.start()..found.start() + remove_len, "");
    }
    path
}

fn short_gen(generics: &str) -> String {
    let mut generics = lifetime_re().replace_all(generics, "").into_owned();
    generics = generics.replace("<>", "");
    generics = generic_leading_comma_re()
        .replace_all(&generics, "<")
        .into_owned();
    generics = generic_trailing_comma_re()
        .replace_all(&generics, ">")
        .into_owned();
    keep2(&generics)
}

fn shorten_inner_gen(path: &str) -> String {
    let Some(generics) = outer_gen(path) else {
        return path.to_string();
    };
    let start = path.find('<').unwrap_or(path.len());
    let end = start + generics.len();
    format!("{}{}{}", &path[..start], short_gen(generics), &path[end..])
}

fn self_generics(signature: &str, self_short: &str) -> String {
    let needle = format!("{self_short}<");
    let Some(start) = signature.find(&needle) else {
        return String::new();
    };
    let suffix = &signature[start + self_short.len()..];
    outer_gen(suffix).map(short_gen).unwrap_or_default()
}

fn trait_disp(trait_path: &str) -> String {
    let Some(generics) = outer_gen(trait_path) else {
        return last_seg(trait_path);
    };
    let start = trait_path.find('<').unwrap_or(trait_path.len());
    format!("{}{}", last_seg(&trait_path[..start]), short_gen(generics))
}

fn strip_attribute(value: &str) -> String {
    attribute_re().replace(value, "").into_owned()
}

fn strip_pub(value: &str) -> String {
    pub_re().replace(value, "").into_owned()
}

fn strip_item_prefix(value: &str) -> String {
    let value = qualifiers_re().replace(value, "");
    item_keyword_re().replace(&value, "").into_owned()
}

fn truncate_item(mut path: String) -> String {
    if let Some(at) = path.find([' ', '(', '=']) {
        path.truncate(at);
    }
    if path.ends_with(':') {
        path.pop();
    }
    path
}

fn group_key(signature: &str) -> String {
    let mut signature = strip_inner_generics(strip_attribute(signature));
    let is_declaration;
    let path;
    if signature.starts_with("impl ") {
        signature = impl_keyword_re().replace(&signature, "").into_owned();
        if let Some(at) = signature.rfind(" for ") {
            signature = signature[at + 5..].to_string();
        }
        path = signature;
        is_declaration = true;
    } else {
        signature = strip_pub(&signature);
        if declaration_re().is_match(&signature) {
            path = declaration_re().replace(&signature, "").into_owned();
            is_declaration = true;
        } else {
            path = strip_item_prefix(&signature);
            is_declaration = false;
        }
    }

    let mut path = truncate_item(path);
    if !is_declaration {
        if let Some(at) = path.rfind("::") {
            path.truncate(at);
        }
    }
    path
}

fn qual_path(signature: &str, crate_prefix: &str) -> String {
    let mut signature = strip_attribute(signature);
    signature = impl_generics_re().replace(&signature, "impl").into_owned();
    if signature.starts_with("impl ") {
        if !crate_prefix.is_empty() {
            signature = signature.replace(&format!("{crate_prefix}::"), "");
        }
        return signature;
    }

    signature = strip_pub(&signature);
    signature = qualifiers_re().replace(&signature, "").into_owned();
    signature = qualified_item_keyword_re()
        .replace(&signature, "")
        .into_owned();
    truncate_item(strip_inner_generics(signature))
}

fn disp(path: &str, crate_prefix: &str) -> String {
    if crate_prefix.is_empty() {
        return path.to_string();
    }
    path.strip_prefix(&format!("{crate_prefix}::"))
        .unwrap_or(path)
        .to_string()
}

fn member_disp(path: &str, group: &str, crate_prefix: &str) -> String {
    if path.starts_with("impl ") {
        return path.to_string();
    }
    if let Some(member) = path.strip_prefix(&format!("{group}::")) {
        return member.to_string();
    }
    disp(path, crate_prefix)
}

fn relsig(signature: &str, crate_prefix: &str) -> String {
    let mut signature = strip_attribute(signature);
    signature = strip_pub(&signature);
    if !crate_prefix.is_empty() {
        signature = signature.replace(&format!("{crate_prefix}::"), "");
    }
    signature
}

fn is_proptest(signature: &str) -> bool {
    signature.contains("proptest::")
        || arbitrary_impl_re().is_match(signature)
        || proptest_assoc_re().is_match(signature)
        || proptest_method_re().is_match(signature)
}

struct ImplGroup {
    key: String,
    is_header: bool,
    header_text: Option<String>,
}

fn impl_group(
    signature: &str,
    group: &str,
    qualified_path: &str,
    crate_prefix: &str,
    traits: &TraitMap,
) -> Option<ImplGroup> {
    if qualified_path.starts_with("impl ") {
        let body = qualified_path.strip_prefix("impl ")?.trim_start();
        let first_for = body.find(" for ")?;
        let last_for = body.rfind(" for ")?;
        let trait_path = &body[..first_for];
        let self_path = &body[last_for + 5..];
        let self_path = shorten_inner_gen(self_path);
        return Some(ImplGroup {
            key: format!("impl {} for {self_path}", last_seg(trait_path)),
            is_header: true,
            header_text: Some(format!("impl {} for {self_path}", trait_disp(trait_path))),
        });
    }

    let self_short = last_seg(group);
    let member = last_seg(qualified_path);
    let trait_path = traits.get(&(self_short.clone(), member))?;
    Some(ImplGroup {
        key: format!(
            "impl {trait_path} for {}{}",
            disp(group, crate_prefix),
            self_generics(signature, &self_short)
        ),
        is_header: false,
        header_text: None,
    })
}

#[derive(Default)]
struct Group {
    key: String,
    is_impl: bool,
    header_text: Option<String>,
    members: Vec<String>,
}

fn render_changed(lines: &[String], crate_prefix: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut pending = String::new();
    let mut skip = false;
    let mut seen = HashSet::new();

    for buffered in lines.iter().filter(|line| !line.trim().is_empty()) {
        if let Some(old) = buffered.strip_prefix("  - ") {
            skip = is_proptest(old);
            pending = relsig(old, crate_prefix);
            continue;
        }
        let Some(new) = buffered.strip_prefix("  + ") else {
            continue;
        };
        if skip {
            skip = false;
            continue;
        }
        let new = relsig(new, crate_prefix);
        if !seen.insert((pending.clone(), new.clone())) {
            continue;
        }
        output.push(format!("- `{pending}`"));
        output.push(format!("  → `{new}`"));
    }
    output
}

fn render_plain(lines: &[String], crate_prefix: &str, traits: &TraitMap) -> Vec<String> {
    let buffered: Vec<&str> = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(String::as_str)
        .collect();

    let mut lazy_static = HashSet::new();
    let mut added_modules = Vec::new();
    for line in &buffered {
        if (line.starts_with("impl<") || line.starts_with("impl "))
            && line.contains("LazyStatic")
            && line.contains(" for ")
        {
            lazy_static.insert(group_key(line));
        }

        let module = strip_attribute(line);
        if pub_module_re().is_match(&module) {
            let module = pub_module_re().replace(&module, "").into_owned();
            if !added_modules.contains(&module) {
                added_modules.push(module);
            }
        }
    }

    let mut groups = Vec::<Group>::new();
    let mut group_indices = HashMap::<String, usize>::new();
    let mut seen_items = HashSet::<(String, String)>::new();

    for signature in buffered {
        let impl_line = signature.starts_with("impl<") || signature.starts_with("impl ");
        if impl_line && !signature.contains(" for ") {
            continue;
        }
        if is_proptest(signature) || structural_eq_re().is_match(signature) {
            continue;
        }

        let original_group = group_key(signature);
        let qualified_path = qual_path(signature, crate_prefix);
        if lazy_static.contains(&original_group) && qualified_path != original_group {
            continue;
        }
        if added_modules.iter().any(|module| {
            qualified_path != *module
                && (original_group == *module || original_group.starts_with(&format!("{module}::")))
        }) {
            continue;
        }
        if !seen_items.insert((original_group.clone(), qualified_path.clone())) {
            continue;
        }

        let impl_info = impl_group(
            signature,
            &original_group,
            &qualified_path,
            crate_prefix,
            traits,
        );
        let key = impl_info
            .as_ref()
            .map_or_else(|| original_group.clone(), |info| info.key.clone());
        let index = if let Some(index) = group_indices.get(&key) {
            *index
        } else {
            let index = groups.len();
            groups.push(Group {
                key: key.clone(),
                ..Group::default()
            });
            group_indices.insert(key, index);
            index
        };
        let group = &mut groups[index];
        if let Some(info) = impl_info {
            group.is_impl = true;
            if info.is_header {
                group.header_text = info.header_text;
            } else {
                group.members.push(last_seg(&qualified_path));
            }
        } else {
            group.members.push(qualified_path);
        }
    }

    emit_groups(groups, crate_prefix)
}

fn emit_groups(groups: Vec<Group>, crate_prefix: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut memberless = Vec::<(String, String)>::new();

    for group in groups {
        let displayed_members: Vec<&str> = group
            .members
            .iter()
            .filter(|member| group.is_impl || member.as_str() != group.key)
            .map(String::as_str)
            .collect();

        if group.is_impl {
            let header = group.header_text.as_deref().unwrap_or(&group.key);
            let kept: Vec<&str> = displayed_members
                .into_iter()
                .filter(|member| !BOILERPLATE.contains(member))
                .collect();
            if kept.is_empty() {
                if let Some(suffix) = header.strip_prefix("impl ") {
                    if let Some((trait_path, self_path)) = suffix.split_once(" for ") {
                        memberless.push((trait_path.to_string(), self_path.to_string()));
                    }
                }
                continue;
            }
            output.push(format!("- `{header}`:"));
            output.extend(kept.into_iter().map(|member| format!("  - `{member}`")));
            continue;
        }

        if group.key == crate_prefix {
            output.extend(
                displayed_members
                    .into_iter()
                    .map(|member| format!("- `{}`", member_disp(member, &group.key, crate_prefix))),
            );
            continue;
        }

        let header = disp(&group.key, crate_prefix);
        match displayed_members.as_slice() {
            [] => output.push(format!("- `{header}`")),
            [member] => output.push(format!(
                "- `{header}::{}`",
                member_disp(member, &group.key, crate_prefix)
            )),
            members => {
                let members = members
                    .iter()
                    .map(|member| member_disp(member, &group.key, crate_prefix))
                    .collect::<Vec<_>>();
                let one_line = format!("- `{header}::{{{}}}`", members.join(", "));
                if one_line.chars().count() <= WIDTH {
                    output.push(one_line);
                } else {
                    output.push(format!("- `{header}`:"));
                    output.extend(members.into_iter().map(|member| format!("  - `{member}`")));
                }
            }
        }
    }

    emit_memberless(memberless, &mut output);
    output
}

fn emit_memberless(memberless: Vec<(String, String)>, output: &mut Vec<String>) {
    let mut by_self = Vec::<(String, Vec<String>)>::new();
    let mut self_indices = HashMap::<String, usize>::new();
    for (trait_path, self_path) in memberless {
        let index = if let Some(index) = self_indices.get(&self_path) {
            *index
        } else {
            let index = by_self.len();
            by_self.push((self_path.clone(), Vec::new()));
            self_indices.insert(self_path, index);
            index
        };
        by_self[index].1.push(trait_path);
    }

    let mut by_trait = Vec::<(String, Vec<String>)>::new();
    let mut trait_indices = HashMap::<String, usize>::new();
    for (self_path, trait_paths) in by_self {
        if trait_paths.len() >= 2 {
            output.push(format!(
                "- `impl {{{}}} for {self_path}`",
                trait_paths.join(", ")
            ));
            continue;
        }
        let Some(trait_path) = trait_paths.into_iter().next() else {
            continue;
        };
        let index = if let Some(index) = trait_indices.get(&trait_path) {
            *index
        } else {
            let index = by_trait.len();
            by_trait.push((trait_path.clone(), Vec::new()));
            trait_indices.insert(trait_path, index);
            index
        };
        by_trait[index].1.push(self_path);
    }

    for (trait_path, self_paths) in by_trait {
        match self_paths.as_slice() {
            [self_path] => output.push(format!("- `impl {trait_path} for {self_path}`")),
            _ => {
                output.push(format!("- `impl {trait_path}` for:"));
                output.extend(
                    self_paths
                        .into_iter()
                        .map(|self_path| format!("  - `{self_path}`")),
                );
            }
        }
    }
}

/// Renders one API diff section as markdown bullet lines.
pub fn render(
    lines: &[String],
    section: Section,
    crate_prefix: &str,
    traits: &TraitMap,
) -> Vec<String> {
    if section == Section::Changed {
        render_changed(lines, crate_prefix)
    } else {
        render_plain(lines, crate_prefix, traits)
    }
}

#[cfg(test)]
mod tests;
