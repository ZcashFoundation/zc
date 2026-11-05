//! Additions that invalidate downstream matches, literals, or externally satisfiable trait impls.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::rustdoc::{id_text, item, public_items};

pub fn breaking_additions(base_json: &Path, head_json: &Path) -> Result<Vec<String>, String> {
    let read = |path: &Path| {
        let json = fs::read_to_string(path)
            .map_err(|err| format!("cannot read rustdoc JSON {}: {err}", path.display()))?;
        crate::rustdoc::parse_json(&json).map_err(|err| format!("{}: {err}", path.display()))
    };
    compare(&read(base_json)?, &read(head_json)?)
}

fn compare(base: &Value, head: &Value) -> Result<Vec<String>, String> {
    let mut base_items = public_items(base).items;
    let head_items = public_items(head).items;
    let mut reachable: HashSet<_> = base_items
        .values()
        .filter_map(|item| item.get("id").and_then(id_text))
        .collect();
    for exported in base_items.values_mut() {
        *exported = resolve_alias(base, exported, &mut reachable);
    }
    let mut changes = BTreeSet::new();
    for (path, old) in &base_items {
        let Some(new) = head_items.get(path) else {
            continue;
        };
        let new = resolve_alias(head, new, &mut HashSet::new());
        if let (Some(old_enum), Some(new_enum)) =
            (old.pointer("/inner/enum"), new.pointer("/inner/enum"))
        {
            let previous: BTreeMap<_, _> = old_enum
                .get("variants")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|id| item(base, id))
                .filter_map(|variant| Some((variant.get("name")?.as_str()?, variant)))
                .collect();
            for variant in new_enum
                .get("variants")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|id| item(head, id))
            {
                let Some(name) = variant.get("name").and_then(Value::as_str) else {
                    continue;
                };
                match previous.get(name) {
                    Some(old_variant) if variant_fields_added(base, old_variant, head, variant) => {
                        changes
                            .insert(format!("{path}::{name}: field added to exhaustive variant"));
                    }
                    None if !non_exhaustive(old) => {
                        changes.insert(format!("{path}::{name}: variant added to exhaustive enum"));
                    }
                    _ => {}
                }
            }
        } else if let (Some(old_struct), Some(new_struct)) =
            (old.pointer("/inner/struct"), new.pointer("/inner/struct"))
        {
            if !non_exhaustive(old) && literal_constructible(base, old_struct) {
                if !literal_constructible(head, new_struct) {
                    changes.insert(format!("{path}: struct is no longer literal-constructible"));
                }
                let previous = names(base, struct_fields(old_struct));
                for field in struct_fields(new_struct)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| item(head, id))
                    .filter(|field| {
                        field.get("visibility").and_then(Value::as_str) == Some("public")
                    })
                {
                    let Some(name) = field.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    if !previous.contains(name) {
                        changes.insert(format!(
                            "{path}::{name}: public field added to constructible struct"
                        ));
                    }
                }
            }
        } else if variant_fields_added(base, old, head, new) {
            changes.insert(format!("{path}: field added to exhaustive variant"));
        } else if let (Some(old_trait), Some(new_trait)) =
            (old.pointer("/inner/trait"), new.pointer("/inner/trait"))
        {
            let previous = names(base, old_trait.get("items"));
            let mut required = new_trait
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|id| item(head, id))
                .filter_map(|method| {
                    let name = method.get("name").and_then(Value::as_str)?;
                    (!previous.contains(name)
                        && method
                            .pointer("/inner/function/has_body")
                            .and_then(Value::as_bool)
                            == Some(false))
                    .then_some(name)
                })
                .peekable();
            if required.peek().is_some()
                && implementable(base, old, &reachable, &mut HashSet::new())?
            {
                for name in required {
                    changes.insert(format!("{path}::{name}: required trait method added"));
                }
            }
        }
    }
    Ok(changes.into_iter().collect())
}

fn resolve_alias<'a>(
    root: &'a Value,
    mut current: &'a Value,
    reachable: &mut HashSet<String>,
) -> &'a Value {
    let mut visiting = HashSet::new();
    while let Some(alias) = current.pointer("/inner/type_alias") {
        // ponytail: direct aliases only; substitute arguments before resolving generic targets.
        if alias
            .pointer("/generics/params")
            .and_then(Value::as_array)
            .is_some_and(|params| !params.is_empty())
        {
            break;
        }
        let Some(path) = alias.pointer("/type/resolved_path") else {
            break;
        };
        if path.get("args").is_some_and(|args| !args.is_null()) {
            break;
        }
        let Some(id) = path.get("id").filter(|id| local(root, id)) else {
            break;
        };
        let Some(key) = id_text(id).filter(|id| visiting.insert(id.clone())) else {
            break;
        };
        let Some(target) = item(root, id) else { break };
        reachable.insert(key);
        current = target;
    }
    current
}

fn non_exhaustive(item: &Value) -> bool {
    item.get("attrs")
        .and_then(Value::as_array)
        .is_some_and(|attrs| {
            attrs
                .iter()
                .any(|attr| matches!(attr.as_str(), Some("non_exhaustive" | "#[non_exhaustive]")))
        })
}

fn names(root: &Value, ids: Option<&Value>) -> BTreeSet<String> {
    ids.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|id| item(root, id))
        .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn struct_fields(struct_item: &Value) -> Option<&Value> {
    struct_item
        .pointer("/kind/plain/fields")
        .or_else(|| struct_item.pointer("/kind/struct/fields"))
        .or_else(|| struct_item.pointer("/kind/tuple"))
}

fn variant_fields_added(base: &Value, old: &Value, head: &Value, new: &Value) -> bool {
    let (Some(old_variant), Some(new_variant)) =
        (old.pointer("/inner/variant"), new.pointer("/inner/variant"))
    else {
        return false;
    };
    !non_exhaustive(old)
        && !names(head, struct_fields(new_variant))
            .is_subset(&names(base, struct_fields(old_variant)))
}

fn literal_constructible(root: &Value, struct_item: &Value) -> bool {
    if struct_item
        .pointer("/kind/plain/has_stripped_fields")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return false;
    }
    if struct_item.get("kind").and_then(Value::as_str) == Some("unit") {
        return true;
    }
    struct_fields(struct_item)
        .and_then(Value::as_array)
        .is_some_and(|fields| {
            fields.iter().all(|id| {
                item(root, id).is_some_and(|field| {
                    field.get("visibility").and_then(Value::as_str) == Some("public")
                })
            })
        })
}

fn local(root: &Value, id: &Value) -> bool {
    let owner = root.get("root").and_then(|id| item(root, id));
    let target = item(root, id).or_else(|| root.get("paths")?.get(id_text(id)?));
    target.and_then(|item| item.get("crate_id")) == owner.and_then(|item| item.get("crate_id"))
}

fn implementable(
    root: &Value,
    trait_item: &Value,
    reachable: &HashSet<String>,
    visiting: &mut HashSet<String>,
) -> Result<bool, String> {
    let Some(id) = trait_item.get("id").and_then(id_text) else {
        return Err("cannot determine implementability of trait without an id".into());
    };
    if !visiting.insert(id.clone()) {
        return Ok(true);
    }
    let result = (|| {
        let Some(trait_def) = trait_item.pointer("/inner/trait") else {
            return Ok(true);
        };
        if !bounds_implementable(root, &trait_def["bounds"], reachable, visiting)? {
            return Ok(false);
        }
        for predicate in trait_def
            .pointer("/generics/where_predicates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|predicate| {
                predicate
                    .pointer("/bound_predicate/type/generic")
                    .and_then(Value::as_str)
                    == Some("Self")
            })
        {
            if !bounds_implementable(root, predicate, reachable, visiting)? {
                return Ok(false);
            }
        }
        Ok(trait_def
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|id| item(root, id))
            .filter(|member| {
                member
                    .pointer("/inner/function/has_body")
                    .and_then(Value::as_bool)
                    == Some(false)
            })
            .all(|member| !private_type(root, &member["inner"]["function"]["sig"], reachable)))
    })();
    visiting.remove(&id);
    result
}

fn bounds_implementable(
    root: &Value,
    value: &Value,
    reachable: &HashSet<String>,
    visiting: &mut HashSet<String>,
) -> Result<bool, String> {
    if let Some(bound) = value.get("trait_bound") {
        // Relaxed bounds do not require the downstream type to implement the trait.
        if bound.get("modifier").and_then(Value::as_str) == Some("maybe") {
            return Ok(true);
        }
        if let Some(id) = bound.pointer("/trait/id") {
            if local(root, id) {
                let trait_item = item(root, id)
                    .ok_or_else(|| format!("cannot resolve local supertrait {id}"))?;
                let exported = id_text(id).is_some_and(|id| reachable.contains(&id));
                if !(exported && implementable(root, trait_item, reachable, visiting)?)
                    && !downstream_blanket_impl(root, trait_item, reachable, visiting)?
                {
                    return Ok(false);
                }
            }
        }
    }
    for value in value.as_array().into_iter().flatten().chain(
        value
            .as_object()
            .into_iter()
            .flat_map(|values| values.values()),
    ) {
        if !bounds_implementable(root, value, reachable, visiting)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn downstream_blanket_impl(
    root: &Value,
    trait_item: &Value,
    reachable: &HashSet<String>,
    visiting: &mut HashSet<String>,
) -> Result<bool, String> {
    let key = format!("blanket:{}", trait_item["id"]);
    if !visiting.insert(key.clone()) {
        // A circular private prerequisite supplies no downstream implementation.
        return Ok(false);
    }
    let mut unsupported = None;
    let mut found = false;
    for implementation in trait_item
        .pointer("/inner/trait/implementations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|id| item(root, id))
        .filter_map(|item| item.pointer("/inner/impl"))
        .filter(|implementation| {
            implementation.get("is_negative").and_then(Value::as_bool) != Some(true)
        })
    {
        match blanket_implementable(root, implementation, reachable, visiting) {
            Ok(true) => {
                found = true;
                break;
            }
            Ok(false) => {}
            Err(error) => unsupported = Some(error),
        }
    }
    visiting.remove(&key);
    if !found {
        if let Some(error) = unsupported {
            return Err(error);
        }
    }
    Ok(found)
}

fn blanket_implementable(
    root: &Value,
    implementation: &Value,
    reachable: &HashSet<String>,
    visiting: &mut HashSet<String>,
) -> Result<bool, String> {
    let unsupported =
        || "cannot determine downstream implementability of complex blanket impl".to_owned();
    let params = implementation
        .pointer("/generics/params")
        .and_then(Value::as_array)
        .ok_or_else(unsupported)?;
    let Some(target) = implementation
        .pointer("/for/generic")
        .and_then(Value::as_str)
    else {
        return if params.is_empty() {
            Ok(false)
        } else {
            Err(unsupported())
        };
    };
    if params.len() != 1 || params[0].get("name").and_then(Value::as_str) != Some(target) {
        return Err(unsupported());
    }
    let bounds = params[0]
        .pointer("/kind/type/bounds")
        .ok_or_else(unsupported)?;
    if !simple_blanket_bounds(bounds) {
        return Err(unsupported());
    }
    if !bounds_implementable(root, bounds, reachable, visiting)? {
        return Ok(false);
    }
    for predicate in implementation
        .pointer("/generics/where_predicates")
        .and_then(Value::as_array)
        .ok_or_else(unsupported)?
    {
        let Some(bound) = predicate.get("bound_predicate") else {
            return Err(unsupported());
        };
        if bound.pointer("/type/generic").and_then(Value::as_str) != Some(target)
            || bound
                .get("generic_params")
                .and_then(Value::as_array)
                .is_some_and(|p| !p.is_empty())
            || !simple_blanket_bounds(&bound["bounds"])
        {
            return Err(unsupported());
        }
        if !bounds_implementable(root, &bound["bounds"], reachable, visiting)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn simple_blanket_bounds(bounds: &Value) -> bool {
    // ponytail: only argument-free trait/lifetime bounds; complex constraints remain analysis errors.
    bounds.as_array().is_some_and(|bounds| {
        bounds.iter().all(|bound| {
            if bound.get("outlives").and_then(Value::as_str).is_some() {
                return true;
            }
            let Some(bound) = bound.get("trait_bound") else {
                return false;
            };
            bound.pointer("/trait/id").is_some()
                && bound.pointer("/trait/args").is_none_or(Value::is_null)
                && bound
                    .get("generic_params")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
        })
    })
}

fn private_type(root: &Value, value: &Value, reachable: &HashSet<String>) -> bool {
    if let Some(id) = value.pointer("/resolved_path/id") {
        if local(root, id) && id_text(id).is_some_and(|id| !reachable.contains(&id)) {
            return true;
        }
    }
    match value {
        Value::Array(values) => values.iter().any(|v| private_type(root, v, reachable)),
        Value::Object(values) => values.values().any(|v| private_type(root, v, reachable)),
        _ => false,
    }
}

#[cfg(test)]
#[path = "compat/tests.rs"]
mod tests;
