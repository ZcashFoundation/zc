//! Reachable public rustdoc items, retaining the producer's crate provenance.

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

/// Parses a rustdoc crate, rejecting unrelated JSON rather than reporting an empty API.
pub fn parse_json(json: &str) -> Result<Value, String> {
    let root: Value =
        serde_json::from_str(json).map_err(|err| format!("invalid rustdoc JSON: {err}"))?;
    let root_id = root.get("root").and_then(id_text);
    let module = root_id
        .as_deref()
        .and_then(|id| root.get("index")?.get(id))
        .filter(|item| {
            item.get("name").is_some_and(Value::is_string)
                && item.get("crate_id").is_some_and(Value::is_u64)
                && item.get("id").and_then(id_text).as_deref() == root_id.as_deref()
        })
        .and_then(|item| item.pointer("/inner/module/items"));
    if !module.is_some_and(Value::is_array) {
        return Err("invalid rustdoc JSON: missing root module/index".to_string());
    }
    Ok(root)
}

pub(crate) fn id_text(id: &Value) -> Option<String> {
    match id {
        Value::String(id) => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

pub(crate) fn item<'a>(root: &'a Value, id: &Value) -> Option<&'a Value> {
    root.get("index")?.get(id_text(id)?)
}

fn public(item: &Value) -> bool {
    item.get("visibility").and_then(Value::as_str) == Some("public")
}

/// Exported display paths and stable implementation-member comparison identities.
#[derive(Clone, Default)]
pub struct PublicItems<'a> {
    /// References retain the original item and its crate provenance.
    pub items: BTreeMap<String, &'a Value>,
    /// Exact impl structure and public-slot candidates, shared by both snapshots when paired.
    pub(crate) impl_identities: BTreeMap<String, crate::traitmap::ImplIdentity>,
    preferred: BTreeMap<String, (bool, String)>,
}

impl<'a> PublicItems<'a> {
    pub(crate) fn preferred_path(&self, id: &Value) -> Option<&str> {
        self.preferred
            .get(&id_text(id)?)
            .map(|(_, path)| path.as_str())
    }

    fn insert(&mut self, path: &str, current: &'a Value, renamed: bool) {
        if let Some(id) = current.get("id").and_then(id_text) {
            // public-api 0.52: unrenamed paths, then segment count. Its reverse-source
            // walk takes the last equal candidate; our forward walk keeps the first.
            let candidate = (renamed, path.matches("::").count());
            let preferred = self
                .preferred
                .entry(id)
                .or_insert_with(|| (renamed, path.into()));
            if candidate < (preferred.0, preferred.1.matches("::").count()) {
                *preferred = (renamed, path.into());
            }
        }
        self.items.insert(path.into(), current);
    }
}

/// Full exported paths, including non-overridden default trait constants.
pub fn public_items(root: &Value) -> PublicItems<'_> {
    public_items_for_comparison(root, public_exports(root), None)
}

pub(crate) fn public_exports(root: &Value) -> PublicItems<'_> {
    let mut output = PublicItems::default();
    let Some(root_item) = root.get("root").and_then(|id| item(root, id)) else {
        return output;
    };
    let Some(name) = root_item.get("name").and_then(Value::as_str) else {
        return output;
    };
    visit(
        root,
        root_item,
        name,
        false,
        &mut HashSet::new(),
        &mut output,
    );
    output
}

pub(crate) fn public_items_for_comparison<'a>(
    root: &'a Value,
    mut output: PublicItems<'a>,
    shared: Option<&HashSet<String>>,
) -> PublicItems<'a> {
    let Some(root_item) = root.get("root").and_then(|id| item(root, id)) else {
        return output;
    };

    // Impl reachability comes from exported owners or exported traits, not impl visibility.
    let owners = output.clone();
    let mut identities = PublicItems::default();
    for (path, item) in &owners.items {
        if shared.is_none_or(|paths| paths.contains(path)) {
            identities.items.insert(path.clone(), *item);
        }
    }
    let mut visited = HashSet::new();
    for (path, owner) in &owners.items {
        let Some(inner) = owner.get("inner").and_then(Value::as_object) else {
            continue;
        };
        for kind in ["struct", "enum", "union", "primitive"] {
            let Some(impls) = inner
                .get(kind)
                .and_then(|v| v.get("impls"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for implementation_item in impls.iter().filter_map(|id| item(root, id)) {
                if let Some(id) = implementation_item.get("id").and_then(id_text) {
                    visited.insert(id);
                }
                add_impl(
                    root,
                    root_item,
                    implementation_item,
                    Some((&owner["id"], path.as_str())),
                    &owners,
                    &identities,
                    &mut output,
                );
            }
        }
    }
    // Trait implementation lists include owners absent from local struct/enum lists,
    // such as Vec<u8>. Use the same member/default traversal as local owners.
    for owner in owners.items.values() {
        for implementation_item in owner
            .pointer("/inner/trait/implementations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|id| item(root, id))
        {
            if implementation_item
                .get("id")
                .and_then(id_text)
                .is_some_and(|id| !visited.insert(id))
            {
                continue;
            }
            add_impl(
                root,
                root_item,
                implementation_item,
                None,
                &owners,
                &identities,
                &mut output,
            );
        }
    }
    output
}

fn public_owner(root: &Value, value: &Value, local: &Value, owners: &PublicItems<'_>) -> bool {
    match value {
        Value::Array(values) => values
            .iter()
            .all(|value| public_owner(root, value, local, owners)),
        Value::Object(values) => {
            let visible = values.get("id").and_then(id_text).is_none_or(|id| {
                root.get("index")
                    .and_then(|index| index.get(&id))
                    .or_else(|| root.get("paths")?.get(&id))
                    .is_none_or(|item| {
                        item.get("crate_id") != Some(local) || owners.preferred.contains_key(&id)
                    })
            });
            visible
                && values
                    .values()
                    .all(|value| public_owner(root, value, local, owners))
        }
        _ => true,
    }
}

fn add_impl<'a>(
    root: &'a Value,
    root_item: &Value,
    implementation_item: &'a Value,
    exported_owner: Option<(&Value, &str)>,
    owners: &PublicItems<'_>,
    identities: &PublicItems<'_>,
    output: &mut PublicItems<'a>,
) {
    let Some(implementation) = implementation_item.pointer("/inner/impl") else {
        return;
    };
    if implementation.get("is_negative").and_then(Value::as_bool) == Some(true)
        || !public_owner(root, &implementation["for"], &root_item["crate_id"], owners)
    {
        return;
    }
    let owner = if let Some((id, path)) = exported_owner {
        implementation
            .get("for")
            .and_then(|ty| crate::traitmap::public_owner_name(root, ty, owners, id, path))
            .or_else(|| {
                implementation
                    .pointer("/for/resolved_path/args")
                    .filter(|args| !args.is_null())
                    .map(|args| format!("{path}<{args}>"))
            })
            .unwrap_or_else(|| path.into())
    } else {
        let Some(owner) = implementation
            .get("for")
            .and_then(|ty| crate::traitmap::public_type_name(root, ty, owners))
        else {
            return;
        };
        owner
    };
    let trait_path = implementation.get("trait").filter(|v| !v.is_null());
    let owner_paths = if let Some(trait_path) = trait_path {
        let Some(id) = trait_path.get("id").and_then(id_text) else {
            return;
        };
        let summary = root.get("paths").and_then(|paths| paths.get(&id));
        let trait_item = root.get("index").and_then(|index| index.get(&id));
        let local_crate = root_item.get("crate_id");
        let provenance = trait_item.or(summary).and_then(|v| v.get("crate_id"));
        if provenance == local_crate && !owners.preferred.contains_key(&id) {
            return;
        }
        crate::traitmap::public_path_names(root, trait_path, owners)
            .unwrap_or_else(|| vec![format!("{trait_path}")])
            .into_iter()
            .map(|name| format!("<{owner} as {name}>"))
            .collect()
    } else {
        vec![owner]
    };
    let canonical_impl = crate::traitmap::impl_identity(root, implementation, identities);
    for owner_path in owner_paths {
        if trait_path.is_some() {
            output.items.insert(owner_path.clone(), implementation_item);
            if let Some(canonical) = &canonical_impl {
                output
                    .impl_identities
                    .insert(owner_path.clone(), canonical.clone());
            }
        }
        children(
            root,
            implementation.get("items"),
            &owner_path,
            trait_path.is_some(),
            false,
            &mut HashSet::new(),
            output,
        );
        if let Some(trait_item) = trait_path
            .and_then(|path| path.get("id"))
            .and_then(|id| item(root, id))
        {
            // Explicit members above win over inherited defaults.
            for member in trait_item
                .pointer("/inner/trait/items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|id| item(root, id))
                .filter(|member| {
                    member
                        .pointer("/inner/assoc_const/value")
                        .is_some_and(|value| !value.is_null())
                })
            {
                if let Some(name) = member.get("name").and_then(Value::as_str) {
                    output
                        .items
                        .entry(format!("{owner_path}::{name}"))
                        .or_insert(member);
                    if let Some(canonical) = &canonical_impl {
                        output
                            .impl_identities
                            .insert(format!("{owner_path}::{name}"), canonical.member(name));
                    }
                }
            }
        }
        if let Some(canonical) = &canonical_impl {
            for member in implementation
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|id| item(root, id))
                .filter(|member| trait_path.is_some() || public(member))
            {
                let Some(name) = member.get("name").and_then(Value::as_str) else {
                    continue;
                };
                output
                    .impl_identities
                    .insert(format!("{owner_path}::{name}"), canonical.member(name));
            }
        }
    }
}

fn children<'a>(
    root: &'a Value,
    ids: Option<&Value>,
    parent: &str,
    implicit: bool,
    renamed: bool,
    stack: &mut HashSet<String>,
    output: &mut PublicItems<'a>,
) {
    let Some(ids) = ids.and_then(Value::as_array) else {
        return;
    };
    for child in ids.iter().filter_map(|id| item(root, id)) {
        if !implicit && !public(child) {
            continue;
        }
        if child.pointer("/inner/use").is_some() {
            export(root, child, parent, renamed, stack, output);
        } else if let Some(name) = child.get("name").and_then(Value::as_str) {
            visit(
                root,
                child,
                &format!("{parent}::{name}"),
                renamed,
                stack,
                output,
            );
        }
    }
}

fn export<'a>(
    root: &'a Value,
    import: &'a Value,
    parent: &str,
    renamed: bool,
    stack: &mut HashSet<String>,
    output: &mut PublicItems<'a>,
) {
    let Some(import) = import.pointer("/inner/use") else {
        return;
    };
    let Some(id) = import.get("id").and_then(id_text) else {
        return;
    };
    let target = root.get("index").and_then(|index| index.get(&id));
    if import.get("is_glob").and_then(Value::as_bool) == Some(true) {
        if !stack.insert(id.clone()) {
            return;
        }
        if let Some(target) = target {
            if let Some(module) = target.pointer("/inner/module") {
                children(
                    root,
                    module.get("items"),
                    parent,
                    false,
                    renamed,
                    stack,
                    output,
                );
            } else if let Some(enumeration) = target.pointer("/inner/enum") {
                children(
                    root,
                    enumeration.get("variants"),
                    parent,
                    true,
                    renamed,
                    stack,
                    output,
                );
            }
        } else if let Some(summary) = root.get("paths").and_then(|paths| paths.get(&id)) {
            if let Some(source) = import.get("source").and_then(Value::as_str) {
                output
                    .items
                    .insert(format!("{parent}::{{{source}::*}}"), summary);
            }
        }
        stack.remove(&id);
    } else if let Some(name) = import.get("name").and_then(Value::as_str) {
        let path = format!("{parent}::{name}");
        if let Some(target) = target {
            visit(root, target, &path, true, stack, output);
        } else if let Some(summary) = root.get("paths").and_then(|paths| paths.get(&id)) {
            output.items.insert(path, summary);
        }
    }
}

fn visit<'a>(
    root: &'a Value,
    current: &'a Value,
    path: &str,
    renamed: bool,
    stack: &mut HashSet<String>,
    output: &mut PublicItems<'a>,
) {
    let Some(id) = current.get("id").and_then(id_text) else {
        return;
    };
    if !stack.insert(id.clone()) {
        return;
    }
    output.insert(path, current, renamed);
    if let Some(module) = current.pointer("/inner/module") {
        children(
            root,
            module.get("items"),
            path,
            false,
            renamed,
            stack,
            output,
        );
    } else if let Some(enumeration) = current.pointer("/inner/enum") {
        children(
            root,
            enumeration.get("variants"),
            path,
            true,
            renamed,
            stack,
            output,
        );
    } else if let Some(trait_item) = current.pointer("/inner/trait") {
        children(
            root,
            trait_item.get("items"),
            path,
            true,
            renamed,
            stack,
            output,
        );
    } else if let Some(struct_item) = current.pointer("/inner/struct") {
        fields(
            root,
            struct_item.get("kind"),
            path,
            false,
            renamed,
            stack,
            output,
        );
    } else if let Some(variant) = current.pointer("/inner/variant") {
        fields(
            root,
            variant.get("kind"),
            path,
            true,
            renamed,
            stack,
            output,
        );
    } else if let Some(union) = current.pointer("/inner/union") {
        children(
            root,
            union.get("fields"),
            path,
            false,
            renamed,
            stack,
            output,
        );
    }
    stack.remove(&id);
}

fn fields<'a>(
    root: &'a Value,
    kind: Option<&Value>,
    path: &str,
    implicit: bool,
    renamed: bool,
    stack: &mut HashSet<String>,
    output: &mut PublicItems<'a>,
) {
    let Some(kind) = kind else { return };
    let ids = kind
        .get("tuple")
        .or_else(|| kind.pointer("/plain/fields"))
        .or_else(|| kind.pointer("/struct/fields"));
    children(root, ids, path, implicit, renamed, stack, output);
}

#[cfg(test)]
#[path = "rustdoc/tests.rs"]
mod tests;
