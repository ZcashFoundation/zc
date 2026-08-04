//! Trait attribution for associated public API items.

use std::collections::{HashMap, HashSet};
use std::fs;

use serde_json::Value;

use crate::ctx::Ctx;
use crate::git;

pub type TraitMap = HashMap<(String, String), String>;

/// Builds per-crate trait maps at one ref, using compatible three-column TSV caches.
pub fn dump(ctx: &Ctx, ref_sha: &str, crates: &[String]) -> HashMap<String, TraitMap> {
    let Ok(sha) = git::rev_parse_verify(ref_sha) else {
        return HashMap::new();
    };
    let target = ctx.tmp.dir.join("trait-target");
    let _ = fs::create_dir_all(&target);
    let worktree = match ctx.tmp.sub("trait") {
        Ok(path) => path,
        Err(_) => return HashMap::new(),
    };
    if git::worktree_add(&worktree, ref_sha).is_err() {
        return HashMap::new();
    }

    let mut maps = HashMap::new();
    for (index, crate_name) in crates.iter().enumerate() {
        let cache_name = format!(
            "{}.{}.{}.traitmap.tsv",
            sha, ctx.cache.script_hash, crate_name
        );
        let cache_path = ctx.cache.path(&cache_name);
        if let Ok(cached) = fs::read_to_string(&cache_path) {
            maps.insert(crate_name.clone(), parse_tsv(&cached));
            continue;
        }

        ctx.progress.set(&format!(
            "--changelog: trait map [{}/{}] {crate_name}",
            index + 1,
            crates.len()
        ));
        if let Ok(path) = crate::api::rustdoc_json(ctx, crate_name, &worktree, &target, &sha) {
            let rows = fs::read_to_string(path)
                .ok()
                .map(|json| extract(&json))
                .unwrap_or_default();
            let tsv = render_tsv(&rows);
            ctx.cache.write_atomic(&cache_name, &tsv);
            maps.insert(crate_name.clone(), rows_to_map(&rows));
        } else {
            maps.insert(crate_name.clone(), TraitMap::new());
        }
    }
    git::worktree_remove(&worktree);
    maps
}

fn extract(json: &str) -> Vec<(String, String, String)> {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(index) = root.get("index").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for item in index.values() {
        let Some(implementation) = item
            .get("inner")
            .and_then(Value::as_object)
            .and_then(|inner| inner.get("impl"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        let Some(trait_item) = implementation.get("trait").filter(|item| !item.is_null()) else {
            continue;
        };
        let Some(trait_path) = trait_item.get("path").and_then(Value::as_str) else {
            continue;
        };
        let Some(self_path) = implementation
            .get("for")
            .and_then(Value::as_object)
            .and_then(|item| item.get("resolved_path"))
            .and_then(Value::as_object)
            .and_then(|path| path.get("path"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let self_short = self_path.rsplit("::").next().unwrap_or(self_path);
        let Some(items) = implementation.get("items").and_then(Value::as_array) else {
            continue;
        };
        for id in items {
            let id = match id {
                Value::String(id) => id.clone(),
                _ => id.to_string(),
            };
            let Some(member) = index
                .get(&id)
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            rows.push((
                self_short.to_string(),
                member.to_string(),
                trait_path.to_string(),
            ));
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

fn rows_to_map(rows: &[(String, String, String)]) -> TraitMap {
    let mut map = TraitMap::new();
    for (self_name, member, trait_path) in rows {
        map.entry((self_name.clone(), member.clone()))
            .or_insert_with(|| trait_path.clone());
    }
    map
}

fn render_tsv(rows: &[(String, String, String)]) -> String {
    let mut output = String::new();
    for (self_name, member, trait_path) in rows {
        output.push_str(self_name);
        output.push('\t');
        output.push_str(member);
        output.push('\t');
        output.push_str(trait_path);
        output.push('\n');
    }
    output
}

fn parse_tsv(contents: &str) -> TraitMap {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for line in contents.lines() {
        let mut fields = line.split('\t');
        let (Some(self_name), Some(member), Some(trait_path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let row = (
            self_name.to_string(),
            member.to_string(),
            trait_path.to_string(),
        );
        if seen.insert(row.clone()) {
            rows.push(row);
        }
    }
    rows_to_map(&rows)
}

#[cfg(test)]
#[path = "traitmap/tests.rs"]
mod tests;
