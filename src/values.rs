//! Public constant, static, and documentation diffs from rustdoc JSON.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

use serde_json::Value;

use crate::ctx::Ctx;
use crate::model::{DocChange, ValueChange};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ValueRow {
    crate_name: String,
    path: String,
    identity: Option<crate::traitmap::ImplIdentity>,
    ty: String,
    value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DocRow {
    crate_name: String,
    path: String,
    identity: Option<crate::traitmap::ImplIdentity>,
    docs: String,
}

enum IndexRow {
    Value(ValueRow),
    Doc(DocRow),
}

/// Computes value and documentation changes in deterministic public-path order.
pub fn diff(ctx: &Ctx, crate_count: usize) -> Result<(Vec<ValueChange>, Vec<DocChange>), String> {
    ctx.progress.start();
    let result = (|| {
        let base = dump_index(ctx, false, crate_count)?;
        let head = dump_index(ctx, true, crate_count)?;
        let mut before = Vec::new();
        let mut after = Vec::new();
        for (crate_name, head_root) in &head {
            let Some(base_root) = base.get(crate_name) else {
                continue;
            };
            let (old, new) = extract_pair(base_root, head_root, crate_name);
            before.extend(old);
            after.extend(new);
        }
        compare(&before, &after)
    })();
    ctx.progress.clear();
    result
}

fn dump_index(
    ctx: &Ctx,
    head: bool,
    crate_count: usize,
) -> Result<BTreeMap<String, Value>, String> {
    let (crates, worktree, target, sha, label) = if head {
        (
            &ctx.head_crates,
            &ctx.head_worktree,
            &ctx.head_target,
            &ctx.refs.head_sha,
            "head",
        )
    } else {
        (
            &ctx.baseline_crates,
            &ctx.baseline_worktree,
            &ctx.baseline_target,
            &ctx.refs.baseline_sha,
            "base",
        )
    };
    let mut roots = BTreeMap::new();
    for (index, (crate_name, _)) in crates
        .iter()
        .filter(|(_, metadata)| metadata.lib_name.is_some())
        .enumerate()
    {
        ctx.progress.set(&format!(
            "--with-values: rustdoc JSON [{label} {}/{}] {crate_name}",
            index + 1,
            crate_count
        ));
        let path = crate::api::rustdoc_json(ctx, crate_name, worktree, target, sha)?;
        let json = fs::read_to_string(&path)
            .map_err(|err| format!("cannot read rustdoc JSON {}: {err}", path.display()))?;
        roots.insert(crate_name.clone(), crate::rustdoc::parse_json(&json)?);
    }
    Ok(roots)
}

fn extract_pair(base: &Value, head: &Value, crate_name: &str) -> (Vec<IndexRow>, Vec<IndexRow>) {
    let old = crate::rustdoc::public_exports(base);
    let new = crate::rustdoc::public_exports(head);
    let shared: HashSet<_> = old
        .items
        .keys()
        .filter(|path| new.items.contains_key(*path))
        .cloned()
        .collect();
    (
        extract_items(
            crate::rustdoc::public_items_for_comparison(base, old, Some(&shared)),
            crate_name,
        ),
        extract_items(
            crate::rustdoc::public_items_for_comparison(head, new, Some(&shared)),
            crate_name,
        ),
    )
}

#[cfg(test)]
fn extract(json: &str, crate_name: &str) -> Result<Vec<IndexRow>, String> {
    let root = crate::rustdoc::parse_json(json)?;
    Ok(extract_items(
        crate::rustdoc::public_items(&root),
        crate_name,
    ))
}

fn extract_items(public: crate::rustdoc::PublicItems<'_>, crate_name: &str) -> Vec<IndexRow> {
    let mut rows = Vec::new();
    let mut impl_identities = public.impl_identities;
    let items = public.items;
    for (path, item) in items {
        let identity = impl_identities.remove(&path);
        let inner = item.get("inner").and_then(Value::as_object);

        if let Some(constant) = inner
            .and_then(|inner| inner.get("constant"))
            .and_then(Value::as_object)
        {
            let ty = constant
                .get("type")
                .map(type_text)
                .unwrap_or_else(|| "null".to_string());
            let value = constant
                .get("const")
                .and_then(Value::as_object)
                .and_then(|value| {
                    value
                        .get("value")
                        .filter(|value| !value.is_null())
                        .or_else(|| value.get("expr").filter(|value| !value.is_null()))
                })
                .map(jq_text)
                .unwrap_or_else(|| "?".to_string());
            rows.push(IndexRow::Value(ValueRow {
                crate_name: crate_name.to_string(),
                path: path.clone(),
                identity: identity.clone(),
                ty,
                value,
            }));
        } else if let Some(constant) = inner
            .and_then(|inner| inner.get("assoc_const"))
            .and_then(Value::as_object)
        {
            // A trait constant without a default has no value to compare.
            if let Some(value) = constant.get("value").filter(|value| !value.is_null()) {
                rows.push(IndexRow::Value(ValueRow {
                    crate_name: crate_name.to_string(),
                    path: path.clone(),
                    identity: identity.clone(),
                    ty: constant
                        .get("type")
                        .map(type_text)
                        .unwrap_or_else(|| "null".to_string()),
                    value: jq_text(value),
                }));
            }
        } else if let Some(static_item) = inner
            .and_then(|inner| inner.get("static"))
            .and_then(Value::as_object)
        {
            let ty = static_item
                .get("type")
                .map(type_text)
                .unwrap_or_else(|| "null".to_string());
            let value = static_item
                .get("expr")
                .filter(|value| !value.is_null())
                .map(jq_text)
                .unwrap_or_else(|| "?".to_string());
            rows.push(IndexRow::Value(ValueRow {
                crate_name: crate_name.to_string(),
                path: path.clone(),
                identity: identity.clone(),
                ty,
                value,
            }));
        }

        // Keep empty docs too: adding or removing docs on a surviving item is a change.
        rows.push(IndexRow::Doc(DocRow {
            crate_name: crate_name.to_string(),
            path,
            identity,
            docs: item
                .get("docs")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }));
    }
    rows
}

fn type_text(value: &Value) -> String {
    value
        .get("primitive")
        .filter(|value| !value.is_null())
        .map(jq_text)
        .or_else(|| {
            value
                .get("resolved_path")
                .and_then(|path| path.get("path").or_else(|| path.get("name")))
                .filter(|value| !value.is_null())
                .map(jq_text)
        })
        .unwrap_or_else(|| value.to_string())
}

fn jq_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

fn compare(
    base: &[IndexRow],
    head: &[IndexRow],
) -> Result<(Vec<ValueChange>, Vec<DocChange>), String> {
    let mut base_values = HashMap::new();
    let mut base_docs = HashMap::new();
    let mut value_origins = HashMap::new();
    let mut doc_origins = HashMap::new();
    let mut value_identities = HashMap::new();
    let mut doc_identities = HashMap::new();
    // ponytail: scan equal-shape impl candidates; index slots if alias-heavy crates make this hot.
    for row in base {
        match row {
            IndexRow::Value(row) => {
                base_values.insert((row.crate_name.as_str(), row.path.as_str()), row);
                if let Some(identity) = &row.identity {
                    value_origins
                        .entry((row.crate_name.as_str(), identity.origin.as_str()))
                        .or_insert(row);
                    value_identities
                        .entry((row.crate_name.as_str(), identity.shape.as_str()))
                        .or_insert_with(Vec::new)
                        .push(row);
                }
            }
            IndexRow::Doc(row) => {
                base_docs.insert((row.crate_name.as_str(), row.path.as_str()), row);
                if let Some(identity) = &row.identity {
                    doc_origins
                        .entry((row.crate_name.as_str(), identity.origin.as_str()))
                        .or_insert(row);
                    doc_identities
                        .entry((row.crate_name.as_str(), identity.shape.as_str()))
                        .or_insert_with(Vec::new)
                        .push(row);
                }
            }
        }
    }

    let mut values = Vec::new();
    let mut docs = Vec::new();
    for row in head {
        match row {
            IndexRow::Value(row) => {
                let direct = base_values
                    .get(&(row.crate_name.as_str(), row.path.as_str()))
                    .copied();
                let candidates = row.identity.as_ref().and_then(|identity| {
                    value_identities.get(&(row.crate_name.as_str(), identity.shape.as_str()))
                });
                let old = direct.or_else(|| {
                    row.identity.as_ref().and_then(|identity| {
                        value_origins
                            .get(&(row.crate_name.as_str(), identity.origin.as_str()))
                            .copied()
                    })
                });
                let mut shared = false;
                match (&row.identity, candidates) {
                    (Some(identity), Some(candidates))
                        if direct.is_none() || identity.has_alternative_slots() =>
                    {
                        for old in candidates {
                            let Some(old_identity) = &old.identity else {
                                continue;
                            };
                            if old.value == row.value {
                                shared |= old_identity.matches(identity);
                                continue;
                            }
                            let Some(path) = old_identity.shared_path(identity) else {
                                continue;
                            };
                            shared = true;
                            if direct.is_some() && path == row.path {
                                continue;
                            }
                            values.push(ValueChange {
                                crate_name: row.crate_name.clone(),
                                path,
                                ty: old.ty.clone(),
                                old: old.value.clone(),
                                new: row.value.clone(),
                            });
                        }
                    }
                    _ => {}
                }
                match old {
                    Some(old) if (direct.is_some() || !shared) && old.value != row.value => {
                        values.push(ValueChange {
                            crate_name: row.crate_name.clone(),
                            path: row.path.clone(),
                            ty: old.ty.clone(),
                            old: old.value.clone(),
                            new: row.value.clone(),
                        });
                    }
                    _ => {}
                }
            }
            IndexRow::Doc(row) => {
                let direct = base_docs
                    .get(&(row.crate_name.as_str(), row.path.as_str()))
                    .copied();
                let candidates = row.identity.as_ref().and_then(|identity| {
                    doc_identities.get(&(row.crate_name.as_str(), identity.shape.as_str()))
                });
                let old = direct.or_else(|| {
                    row.identity.as_ref().and_then(|identity| {
                        doc_origins
                            .get(&(row.crate_name.as_str(), identity.origin.as_str()))
                            .copied()
                    })
                });
                let mut shared = false;
                match (&row.identity, candidates) {
                    (Some(identity), Some(candidates))
                        if direct.is_none() || identity.has_alternative_slots() =>
                    {
                        for old in candidates {
                            let Some(old_identity) = &old.identity else {
                                continue;
                            };
                            if old.docs == row.docs {
                                shared |= old_identity.matches(identity);
                                continue;
                            }
                            let Some(path) = old_identity.shared_path(identity) else {
                                continue;
                            };
                            shared = true;
                            if direct.is_some() && path == row.path {
                                continue;
                            }
                            docs.push(DocChange {
                                crate_name: row.crate_name.clone(),
                                path,
                            });
                        }
                    }
                    _ => {}
                }
                if (direct.is_some() || !shared) && old.is_some_and(|old| old.docs != row.docs) {
                    docs.push(DocChange {
                        crate_name: row.crate_name.clone(),
                        path: row.path.clone(),
                    });
                }
            }
        }
    }
    values.sort_by(|a, b| (&a.crate_name, &a.path).cmp(&(&b.crate_name, &b.path)));
    for pair in values.windows(2) {
        if pair[0].crate_name == pair[1].crate_name
            && pair[0].path == pair[1].path
            && (pair[0].old != pair[1].old
                || pair[0].new != pair[1].new
                || pair[0].ty != pair[1].ty)
        {
            return Err(format!("ambiguous value comparison for {}", pair[0].path));
        }
    }
    values.dedup_by(|a, b| a.crate_name == b.crate_name && a.path == b.path);
    docs.sort_by(|a, b| (&a.crate_name, &a.path).cmp(&(&b.crate_name, &b.path)));
    docs.dedup_by(|a, b| a.crate_name == b.crate_name && a.path == b.path);
    Ok((values, docs))
}

#[cfg(test)]
#[path = "values/tests.rs"]
mod tests;
