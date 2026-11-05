//! Keep a Changelog rendering for public API diff lines.

use std::collections::{HashMap, HashSet};

use crate::group::{item_path, without_attributes};
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
    let body = without_attributes(signature);
    let attributes = &signature[..signature.len() - body.len()];
    let body = body.strip_prefix("pub ").unwrap_or(body);
    let body = if crate_prefix.is_empty() {
        body.to_string()
    } else {
        body.replace(&format!("{crate_prefix}::"), "")
    };
    format!("{attributes}{body}")
}

fn impl_body(signature: &str) -> Option<(&str, &str)> {
    let signature = without_attributes(signature).strip_prefix("impl")?;
    if let Some(generics) = signature.strip_prefix('<') {
        let mut depth = 1usize;
        for (index, byte) in generics.bytes().enumerate() {
            match byte {
                b'<' => depth += 1,
                b'>' if index == 0 || generics.as_bytes()[index - 1] != b'-' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((&signature[..index + 2], generics[index + 1..].trim_start()));
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    Some(("", signature.trim_start()))
}

// A where-clause constrains an impl, but is not part of its owner's identity.
fn split_where(body: &str) -> (&str, &str) {
    let mut depth = 0usize;
    let mut braces = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in body.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' => braces += 1,
            b'}' => braces = braces.saturating_sub(1),
            b'<' | b'(' | b'[' if braces == 0 => depth += 1,
            b'>' | b')' | b']'
                if braces == 0 && (index == 0 || body.as_bytes()[index - 1] != b'-') =>
            {
                depth = depth.saturating_sub(1);
            }
            b' ' if depth == 0 && braces == 0 && body[index..].starts_with(" where ") => {
                return (&body[..index], &body[index..]);
            }
            _ => {}
        }
    }
    (body, "")
}

fn qual_path(signature: &str) -> String {
    if impl_body(signature).is_some() {
        without_attributes(signature).to_string()
    } else {
        item_path(signature).0
    }
}

fn group_key(signature: &str) -> String {
    if let Some((_, body)) = impl_body(signature) {
        let (body, _) = split_where(body);
        return body
            .rsplit_once(" for ")
            .map_or(body, |(_, owner)| owner)
            .to_string();
    }
    let (path, declaration) = item_path(signature);
    if declaration {
        path
    } else {
        member_owner(&path).map_or_else(|| path.clone(), |(owner, _)| owner.to_string())
    }
}

fn member_owner(path: &str) -> Option<(&str, &str)> {
    let mut path = path;
    if path.ends_with('>') {
        let mut depth = 0usize;
        for (index, byte) in path.bytes().enumerate().rev() {
            match byte {
                b'>' if index == 0 || path.as_bytes()[index - 1] != b'-' => depth += 1,
                b'<' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        path = &path[..index];
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    path.rsplit_once("::")
}

fn attribution<'a>(path: &str, traits: &'a TraitMap) -> Option<&'a str> {
    let (owner, member) = member_owner(path)?;
    traits
        .get(&(owner.to_string(), member.to_string()))?
        .as_deref()
}

fn is_proptest(signature: &str, traits: &TraitMap) -> bool {
    let trait_path = impl_body(signature)
        .and_then(|(_, body)| body.rsplit_once(" for ").map(|(path, _)| path))
        .or_else(|| attribution(&item_path(signature).0, traits));
    trait_path.is_some_and(|path| {
        path == "proptest::arbitrary::Arbitrary"
            || path.starts_with("proptest::arbitrary::Arbitrary<")
    })
}

fn trait_disp(path: &str, crate_prefix: &str) -> String {
    for prefix in ["core::", "std::", "alloc::"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            let (name, args) = rest
                .find(['<', '('])
                .map_or((rest, ""), |at| rest.split_at(at));
            return format!("{}{args}", name.rsplit("::").next().unwrap_or(name));
        }
    }
    disp(path, crate_prefix)
}

struct ImplGroup {
    key: String,
    is_header: bool,
    header_text: Option<String>,
}

fn impl_group(
    signature: &str,
    qualified_path: &str,
    crate_prefix: &str,
    traits: &TraitMap,
) -> Option<ImplGroup> {
    if let Some((generics, body)) = impl_body(signature) {
        let (body, where_clause) = split_where(body);
        let (trait_path, self_path) = body.rsplit_once(" for ")?;
        return Some(ImplGroup {
            key: format!("impl {trait_path} for {self_path}"),
            is_header: true,
            header_text: Some(format!(
                "impl{generics} {} for {}{where_clause}",
                trait_disp(trait_path, crate_prefix),
                disp(self_path, crate_prefix),
            )),
        });
    }

    let trait_path = attribution(qualified_path, traits)?;
    let (owner, _) = member_owner(qualified_path)?;
    Some(ImplGroup {
        key: format!("impl {trait_path} for {owner}"),
        is_header: false,
        header_text: Some(format!(
            "impl {} for {}",
            trait_disp(trait_path, crate_prefix),
            disp(owner, crate_prefix),
        )),
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
    let mut seen = HashSet::new();

    for buffered in lines.iter().filter(|line| !line.trim().is_empty()) {
        if let Some(old) = buffered.strip_prefix("  - ") {
            pending = relsig(old, crate_prefix);
            continue;
        }
        let Some(new) = buffered.strip_prefix("  + ") else {
            continue;
        };
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
        if impl_body(line).is_some_and(|(_, body)| {
            body.starts_with("lazy_static::") && body.contains("LazyStatic for ")
        }) {
            lazy_static.insert(group_key(line));
        }

        if let Some(module) = without_attributes(line).strip_prefix("pub mod ") {
            if !added_modules.iter().any(|existing| existing == module) {
                added_modules.push(module.to_string());
            }
        }
    }

    let mut groups = Vec::<Group>::new();
    let mut group_indices = HashMap::<String, usize>::new();
    let mut seen_items = HashSet::<(String, String)>::new();

    for signature in buffered {
        let impl_line = impl_body(signature);
        if impl_line.is_some_and(|(_, body)| !body.contains(" for ")) {
            continue;
        }
        if is_proptest(signature, traits)
            || impl_line.is_some_and(|(_, body)| {
                body.starts_with("core::marker::StructuralPartialEq for ")
                    || body.starts_with("core::marker::StructuralEq for ")
            })
        {
            continue;
        }

        let original_group = group_key(signature);
        let qualified_path = qual_path(signature);
        if lazy_static.contains(&original_group) && qualified_path != original_group {
            continue;
        }
        if added_modules.iter().any(|module| {
            qualified_path != *module
                && (original_group == *module || original_group.starts_with(&format!("{module}::")))
        }) {
            continue;
        }
        if !seen_items.insert((original_group.clone(), signature.to_string())) {
            continue;
        }

        let impl_info = impl_group(signature, &qualified_path, crate_prefix, traits);
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
            if info.is_header || group.header_text.is_none() {
                group.header_text = info.header_text;
            }
            if !info.is_header {
                let member = member_owner(&qualified_path)
                    .map_or(qualified_path.as_str(), |(owner, _)| {
                        &qualified_path[owner.len() + 2..]
                    })
                    .to_string();
                if !group.members.contains(&member) {
                    group.members.push(member);
                }
            }
        } else if !group.members.contains(&qualified_path) {
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
                } else {
                    output.push(format!("- `{header}`"));
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
