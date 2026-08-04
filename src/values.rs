//! Public constant, static, and documentation diffs from rustdoc JSON.

use std::collections::HashMap;
use std::fs;

use serde::de::{MapAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;

use crate::cargo_meta::workspace_crate_names;
use crate::ctx::Ctx;
use crate::git;
use crate::model::{DocChange, ValueChange};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ValueRow {
    crate_name: String,
    path: String,
    ty: String,
    value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DocRow {
    crate_name: String,
    path: String,
    docs: String,
}

#[derive(Default)]
struct OrderedEntries(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for OrderedEntries {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct EntriesVisitor;

        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = OrderedEntries;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(OrderedEntries(entries))
            }
        }

        deserializer.deserialize_map(EntriesVisitor)
    }
}

#[derive(Deserialize, Default)]
struct RustdocIndex {
    #[serde(default)]
    index: HashMap<String, Value>,
    #[serde(default)]
    paths: OrderedEntries,
}

enum IndexRow {
    Value(ValueRow),
    Doc(DocRow),
}

/// Computes value and documentation changes, preserving the head rustdoc path order.
pub fn diff(ctx: &Ctx, crate_count: usize) -> (Vec<ValueChange>, Vec<DocChange>) {
    let target = ctx.tmp.dir.join("values-target");
    let _ = fs::create_dir_all(&target);

    ctx.progress.start();
    let base = dump_index(ctx, &ctx.refs.baseline_sha, "base", crate_count, &target);
    let head = dump_index(ctx, &ctx.refs.head_sha, "head", crate_count, &target);
    ctx.progress.clear();
    compare(&base, &head)
}

fn dump_index(
    ctx: &Ctx,
    ref_sha: &str,
    ref_label: &str,
    crate_count: usize,
    target: &std::path::Path,
) -> Vec<IndexRow> {
    let cache_name = format!("{}.{}.values.tsv", ref_sha, ctx.cache.script_hash);
    if let Some(cached) = ctx.cache.read_if_present(&cache_name) {
        return parse_tsv(&cached);
    }

    let worktree = match ctx.tmp.sub("values") {
        Ok(path) => path,
        Err(_) => return Vec::new(),
    };
    if git::worktree_add(&worktree, ref_sha).is_err() {
        eprintln!(
            "{}warning:{} could not create worktree for '{}' (value diff)",
            ctx.style.yellow, ctx.style.reset, ref_sha
        );
        return Vec::new();
    }

    let crates = workspace_crate_names(&worktree);
    let mut rows = Vec::new();
    let mut failed = false;
    for (index, crate_name) in crates.iter().enumerate() {
        ctx.progress.set(&format!(
            "--with-values: rustdoc JSON [{ref_label} {}/{}] {crate_name}",
            index + 1,
            crate_count
        ));
        match crate::api::rustdoc_json(ctx, crate_name, &worktree, target, ref_sha) {
            Ok(path) => {
                if let Ok(json) = fs::read_to_string(path) {
                    rows.extend(extract(&json, crate_name));
                }
            }
            Err(_) => failed = true,
        }
    }
    git::worktree_remove(&worktree);

    let tsv = render_tsv(&rows);
    if !failed {
        ctx.cache.write_atomic(&cache_name, &tsv);
    }
    parse_tsv(&tsv)
}

fn extract(json: &str, crate_name: &str) -> Vec<IndexRow> {
    let Ok(root) = serde_json::from_str::<RustdocIndex>(json) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for (id, path_entry) in root.paths.0 {
        let Some(item) = root.index.get(&id) else {
            continue;
        };
        if item.get("visibility").and_then(Value::as_str) != Some("public") {
            continue;
        }
        let Some(path) = path_entry.get("path").and_then(Value::as_array) else {
            continue;
        };
        let Some(path) = path
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|segments| segments.join("::"))
        else {
            continue;
        };
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
                ty,
                value,
            }));
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
                ty,
                value,
            }));
        }

        if let Some(docs) = item
            .get("docs")
            .and_then(Value::as_str)
            .filter(|docs| !docs.is_empty())
        {
            rows.push(IndexRow::Doc(DocRow {
                crate_name: crate_name.to_string(),
                path,
                docs: base64(docs.as_bytes()),
            }));
        }
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
                .and_then(|path| path.get("name"))
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

fn compare(base: &[IndexRow], head: &[IndexRow]) -> (Vec<ValueChange>, Vec<DocChange>) {
    let mut base_values = HashMap::new();
    let mut base_docs = HashMap::new();
    for row in base {
        match row {
            IndexRow::Value(row) => {
                base_values.insert(
                    (row.crate_name.as_str(), row.path.as_str()),
                    (row.ty.as_str(), row.value.as_str()),
                );
            }
            IndexRow::Doc(row) => {
                base_docs.insert(
                    (row.crate_name.as_str(), row.path.as_str()),
                    row.docs.as_str(),
                );
            }
        }
    }

    let mut values = Vec::new();
    let mut docs = Vec::new();
    for row in head {
        match row {
            IndexRow::Value(row) => {
                if let Some((ty, old)) =
                    base_values.get(&(row.crate_name.as_str(), row.path.as_str()))
                {
                    if *old != row.value {
                        values.push(ValueChange {
                            crate_name: row.crate_name.clone(),
                            path: row.path.clone(),
                            ty: (*ty).to_string(),
                            old: (*old).to_string(),
                            new: row.value.clone(),
                        });
                    }
                }
            }
            IndexRow::Doc(row) => {
                if let Some(old) = base_docs.get(&(row.crate_name.as_str(), row.path.as_str())) {
                    if *old != row.docs {
                        docs.push(DocChange {
                            crate_name: row.crate_name.clone(),
                            path: row.path.clone(),
                        });
                    }
                }
            }
        }
    }
    (values, docs)
}

fn render_tsv(rows: &[IndexRow]) -> String {
    let mut output = String::new();
    for row in rows {
        let fields: Vec<&str> = match row {
            IndexRow::Value(row) => vec!["V", &row.crate_name, &row.path, &row.ty, &row.value],
            IndexRow::Doc(row) => vec!["D", &row.crate_name, &row.path, &row.docs],
        };
        output.push_str(
            &fields
                .into_iter()
                .map(tsv_escape)
                .collect::<Vec<_>>()
                .join("\t"),
        );
        output.push('\n');
    }
    output
}

fn parse_tsv(contents: &str) -> Vec<IndexRow> {
    let mut rows = Vec::new();
    for line in contents.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["V", crate_name, path, ty, value] => rows.push(IndexRow::Value(ValueRow {
                crate_name: (*crate_name).to_string(),
                path: (*path).to_string(),
                ty: (*ty).to_string(),
                value: (*value).to_string(),
            })),
            ["D", crate_name, path, docs] => rows.push(IndexRow::Doc(DocRow {
                crate_name: (*crate_name).to_string(),
                path: (*path).to_string(),
                docs: (*docs).to_string(),
            })),
            _ => {}
        }
    }
    rows
}

fn tsv_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(TABLE[((bits >> 18) & 63) as usize] as char);
        output.push(TABLE[((bits >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((bits >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(bits & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
#[path = "values/tests.rs"]
mod tests;
