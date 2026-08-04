//! Public API diff grouping by owning type or module.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::model::{GroupMode, GroupRecord, Section};

struct Patterns {
    attribute: Regex,
    generic: Regex,
    impl_prefix: Regex,
    pub_prefix: Regex,
    declaration: Regex,
    qualifiers: Regex,
    item_keyword: Regex,
    truncate: Regex,
    member_leaf: Regex,
    type_segment: Regex,
    declaration_kind: Regex,
    member_keyword: Regex,
    upper_camel: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        attribute: Regex::new(r"^#\[[^]]*\] +").expect("valid attribute regex"),
        generic: Regex::new(r"<[^<>]*>").expect("valid generic regex"),
        impl_prefix: Regex::new(r"^impl +").expect("valid impl regex"),
        pub_prefix: Regex::new(r"^pub +").expect("valid visibility regex"),
        declaration: Regex::new(r"^(mod|struct|enum|trait|union) +")
            .expect("valid declaration regex"),
        qualifiers: Regex::new(r"^(?:(?:const|async|unsafe) +)*").expect("valid qualifier regex"),
        item_keyword: Regex::new(r"^(?:fn|static|type|use) +").expect("valid item keyword regex"),
        truncate: Regex::new(r"[ (=].*").expect("valid truncation regex"),
        member_leaf: Regex::new(r"::[^:]+$").expect("valid member regex"),
        type_segment: Regex::new(r"::[A-Z][^:]*$").expect("valid type segment regex"),
        declaration_kind: Regex::new(r"^pub (mod|struct|enum|trait|union) ")
            .expect("valid declaration kind regex"),
        member_keyword: Regex::new(
            r"^pub (?:mod|struct|enum|trait|union|fn|const|static|type|use|async) ",
        )
        .expect("valid member keyword regex"),
        upper_camel: Regex::new(r"^[A-Z]").expect("valid UpperCamelCase regex"),
    })
}

fn without_attribute(signature: &str) -> String {
    patterns().attribute.replace(signature, "").into_owned()
}

fn group_key(signature: &str) -> String {
    let regexes = patterns();
    let mut signature = without_attribute(signature);

    loop {
        let stripped = regexes.generic.replace_all(&signature, "").into_owned();
        if stripped == signature {
            break;
        }
        signature = stripped;
    }

    let (mut key, declaration) = if regexes.impl_prefix.is_match(&signature) {
        let signature = regexes.impl_prefix.replace(&signature, "").into_owned();
        let key = signature
            .rfind(" for ")
            .map_or(signature.as_str(), |index| {
                &signature[index + " for ".len()..]
            });
        (key.to_string(), true)
    } else {
        let mut signature = regexes.pub_prefix.replace(&signature, "").into_owned();
        let declaration = regexes.declaration.is_match(&signature);
        if declaration {
            signature = regexes.declaration.replace(&signature, "").into_owned();
        } else {
            signature = regexes.qualifiers.replace(&signature, "").into_owned();
            signature = regexes.item_keyword.replace(&signature, "").into_owned();
        }
        (signature, declaration)
    };

    key = regexes.truncate.replace(&key, "").into_owned();
    if key.ends_with(':') {
        key.pop();
    }
    if !declaration && key.contains("::") {
        key = regexes.member_leaf.replace(&key, "").into_owned();
    }
    key
}

fn mod_of(key: &str) -> String {
    let mut module = key.to_string();
    loop {
        let stripped = patterns().type_segment.replace(&module, "").into_owned();
        if stripped == module {
            return module;
        }
        module = stripped;
    }
}

fn decl_kind(signature: &str) -> Option<String> {
    let signature = without_attribute(signature);
    patterns()
        .declaration_kind
        .captures(&signature)
        .and_then(|captures| captures.get(1))
        .map(|kind| kind.as_str().to_string())
}

fn member_kind(signature: &str) -> Option<String> {
    let signature = without_attribute(signature);
    if !signature.starts_with("pub ") || patterns().member_keyword.is_match(&signature) {
        return None;
    }
    Some(if signature.contains(": ") {
        "struct".to_string()
    } else {
        "enum".to_string()
    })
}

fn fallback_kind(key: &str) -> String {
    let leaf = key.rsplit("::").next().unwrap_or(key);
    if patterns().upper_camel.is_match(leaf) {
        "type".to_string()
    } else {
        "mod".to_string()
    }
}

/// Kind sources for a type key, in precedence order.
struct Kinds<'a> {
    /// Kinds read from a declaration present in the diff.
    declared: HashMap<String, String>,
    /// Kinds read from the head source, keyed by short name.
    src: &'a HashMap<String, String>,
    /// Kinds weakly inferred from a member.
    member: HashMap<String, String>,
}

impl Kinds<'_> {
    /// Resolved kind: declared in the diff > head source by short name > inferred from a
    /// member > naming convention.
    fn resolve(&self, key: &str) -> String {
        if let Some(kind) = self.declared.get(key) {
            return kind.clone();
        }
        let short_name = key.rsplit("::").next().unwrap_or(key);
        if let Some(kind) = self.src.get(short_name) {
            return kind.clone();
        }
        self.member
            .get(key)
            .cloned()
            .unwrap_or_else(|| fallback_kind(key))
    }
}

fn is_ext(key: &str, crate_prefix: &str) -> bool {
    !crate_prefix.is_empty()
        && key != crate_prefix
        && !key
            .strip_prefix(crate_prefix)
            .is_some_and(|suffix| suffix.starts_with("::"))
}

struct Line<'a> {
    text: &'a str,
    key: String,
    module: String,
}

fn emit_type(
    records: &mut Vec<GroupRecord>,
    key: &str,
    members: &[usize],
    lines: &[Line<'_>],
    kinds: &Kinds<'_>,
) {
    records.push(GroupRecord::TypeHeader {
        name: key.to_string(),
        kind: kinds.resolve(key),
    });

    let module = key.rfind("::").map_or(key, |index| &key[..index]);
    let module_prefix = format!("{module}::");
    for &index in members {
        records.push(GroupRecord::Item(
            lines[index].text.replace(&module_prefix, ""),
        ));
    }
}

fn emit_module(
    records: &mut Vec<GroupRecord>,
    module: &str,
    types: &[String],
    members: &HashMap<String, Vec<usize>>,
    lines: &[Line<'_>],
    kinds: &Kinds<'_>,
) {
    records.push(GroupRecord::ModHeader(module.to_string()));
    let module_prefix = format!("{module}::");

    for key in types {
        let Some(type_members) = members.get(key) else {
            continue;
        };
        if key == module {
            for &index in type_members {
                let mut display = lines[index].text.replace(&module_prefix, "");
                if display.starts_with("pub mod ") {
                    let name = display
                        .rsplit_once("::")
                        .map_or(display.as_str(), |(_, name)| name);
                    display = format!("pub mod {name}");
                }
                records.push(GroupRecord::Item(display));
            }
            continue;
        }

        let relative_name = key.strip_prefix(&module_prefix).unwrap_or(key).to_string();
        records.push(GroupRecord::TypeSub {
            name: relative_name,
            kind: kinds.resolve(key),
        });
        let owner_pattern = format!(r"{}(?:<[^<>]*>)?::", regex::escape(key));
        let owner_regex = Regex::new(&owner_pattern).ok();
        for &index in type_members {
            let display = owner_regex.as_ref().map_or_else(
                || lines[index].text.to_string(),
                |regex| regex.replace_all(lines[index].text, "").into_owned(),
            );
            records.push(GroupRecord::DeepItem(display.replace(&module_prefix, "")));
        }
    }
}

/// Groups public API signatures while preserving each key's first appearance.
pub fn group(
    lines: &[String],
    section: Section,
    mode: GroupMode,
    crate_prefix: &str,
    src_kinds: &HashMap<String, String>,
) -> Vec<GroupRecord> {
    if mode == GroupMode::Flat {
        return Vec::new();
    }

    let mut grouped_lines = Vec::new();
    let mut declared_kinds = HashMap::new();
    let mut member_kinds = HashMap::new();
    let mut changed_key = String::new();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        let (key, declared_kind, inferred_kind) = if section == Section::Changed {
            if let Some(signature) = line.strip_prefix("  - ") {
                changed_key = group_key(signature);
                (
                    changed_key.clone(),
                    decl_kind(signature),
                    member_kind(signature),
                )
            } else {
                (changed_key.clone(), None, None)
            }
        } else {
            (group_key(line), decl_kind(line), member_kind(line))
        };
        let module = mod_of(&key);
        if let Some(kind) = declared_kind {
            declared_kinds.insert(key.clone(), kind);
        }
        if let Some(kind) = inferred_kind {
            member_kinds.entry(key.clone()).or_insert(kind);
        }
        grouped_lines.push(Line {
            text: line,
            key,
            module,
        });
    }

    let kinds = Kinds {
        declared: declared_kinds,
        src: src_kinds,
        member: member_kinds,
    };

    let mut records = Vec::new();
    let mut members: HashMap<String, Vec<usize>> = HashMap::new();

    if mode == GroupMode::Type {
        let mut type_order = Vec::new();
        for (index, line) in grouped_lines.iter().enumerate() {
            if !members.contains_key(&line.key) {
                type_order.push(line.key.clone());
            }
            members.entry(line.key.clone()).or_default().push(index);
        }

        for key in type_order.iter().filter(|key| !is_ext(key, crate_prefix)) {
            emit_type(&mut records, key, &members[key], &grouped_lines, &kinds);
        }
        let external: Vec<&String> = type_order
            .iter()
            .filter(|key| is_ext(key, crate_prefix))
            .collect();
        if !external.is_empty() {
            records.push(GroupRecord::ExtDivider);
        }
        for key in external {
            emit_type(&mut records, key, &members[key], &grouped_lines, &kinds);
        }
        return records;
    }

    let mut module_order = Vec::new();
    let mut module_types: HashMap<String, Vec<String>> = HashMap::new();
    for (index, line) in grouped_lines.iter().enumerate() {
        if !module_types.contains_key(&line.module) {
            module_order.push(line.module.clone());
            module_types.insert(line.module.clone(), Vec::new());
        }
        if !members.contains_key(&line.key) {
            module_types
                .entry(line.module.clone())
                .or_default()
                .push(line.key.clone());
        }
        members.entry(line.key.clone()).or_default().push(index);
    }

    for module in module_order
        .iter()
        .filter(|module| !is_ext(module, crate_prefix))
    {
        emit_module(
            &mut records,
            module,
            &module_types[module],
            &members,
            &grouped_lines,
            &kinds,
        );
    }
    let external: Vec<&String> = module_order
        .iter()
        .filter(|module| is_ext(module, crate_prefix))
        .collect();
    if !external.is_empty() {
        records.push(GroupRecord::ExtDivider);
    }
    for module in external {
        emit_module(
            &mut records,
            module,
            &module_types[module],
            &members,
            &grouped_lines,
            &kinds,
        );
    }

    records
}

#[cfg(test)]
#[path = "group/tests.rs"]
mod tests;
