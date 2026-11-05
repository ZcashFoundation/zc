//! Resolved dependency changes from Cargo.lock.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::ctx::Ctx;
use crate::model::LockDiff;

/// Compare the lock files at both refs, suppressing direct dependencies.
pub fn diff(ctx: &Ctx, direct_names: &HashSet<String>) -> Result<LockDiff, String> {
    diff_worktrees(&ctx.baseline_worktree, &ctx.head_worktree, direct_names)
}

fn diff_worktrees(
    baseline: &Path,
    head: &Path,
    direct_names: &HashSet<String>,
) -> Result<LockDiff, String> {
    let read_lock = |dir: &Path| {
        let path = dir.join("Cargo.lock");
        let contents = fs::read_to_string(&path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let packages = extract_lock(&contents);
        if packages.is_empty() {
            return Err(format!("no packages found in {}", path.display()));
        }
        Ok(packages)
    };
    let base = read_lock(baseline)?;
    let head_lock = read_lock(head)?;

    let output = Command::new("cargo")
        .current_dir(head)
        .args([
            "tree",
            "--locked",
            "--prefix=depth",
            "--edges=normal",
            "--workspace",
            "--no-dedupe",
        ])
        .output()
        .map_err(|err| format!("failed to run cargo tree: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo tree failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let tree = String::from_utf8(output.stdout)
        .map_err(|err| format!("invalid cargo tree output: {err}"))?;
    if !tree.lines().any(|line| tree_entry(line).is_some()) {
        return Err("cargo tree returned no packages".to_string());
    }

    Ok(build_diff(&base, &head_lock, direct_names, &tree))
}

fn extract_lock(contents: &str) -> Vec<(String, String)> {
    let mut packages = BTreeSet::new();
    let mut in_package = false;
    let mut name = String::new();
    let mut version = String::new();

    for line in contents.split('\n') {
        if line.starts_with("[[package]]") {
            name.clear();
            version.clear();
            in_package = true;
            continue;
        }
        if in_package {
            if let Some(value) = quoted_assignment(line, "name") {
                name = value.to_string();
            }
            if let Some(value) = quoted_assignment(line, "version") {
                version = value.to_string();
            }
            if line.is_empty() {
                emit_package(&mut packages, &name, &version);
                name.clear();
                version.clear();
                in_package = false;
            }
        }
    }

    if in_package {
        emit_package(&mut packages, &name, &version);
    }

    packages.into_iter().collect()
}

fn quoted_assignment<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = line.strip_prefix(key)?;
    rest = rest.trim_start_matches(' ');
    rest = rest.strip_prefix('=')?;
    rest = rest.trim_start_matches(' ');
    rest = rest.strip_prefix('"')?;
    Some(rest.strip_suffix('"').unwrap_or(rest))
}

fn emit_package(packages: &mut BTreeSet<(String, String)>, name: &str, version: &str) {
    if !name.is_empty() && !version.is_empty() {
        packages.insert((name.to_string(), version.to_string()));
    }
}

fn build_diff(
    base: &[(String, String)],
    head: &[(String, String)],
    direct_names: &HashSet<String>,
    tree: &str,
) -> LockDiff {
    let base = versions_by_name(base);
    let head = versions_by_name(head);
    let mut result = LockDiff::default();

    for (name, old) in &base {
        if direct_names.contains(name) {
            continue;
        }
        match head.get(name) {
            None => result.removed.push((name.clone(), old.clone())),
            Some(new) if old != new => {
                result
                    .changed
                    .push((name.clone(), old.clone(), new.clone()));
            }
            Some(_) => {}
        }
    }

    for (name, versions) in &head {
        if !direct_names.contains(name) && !base.contains_key(name) {
            result.added.push((name.clone(), versions.clone()));
        }
    }

    result.removed.sort_by(|a, b| a.0.cmp(&b.0));
    result.changed.sort_by(|a, b| a.0.cmp(&b.0));
    result.added.sort_by(|a, b| a.0.cmp(&b.0));
    result.via = attribution(tree, direct_names);
    result
}

fn versions_by_name(packages: &[(String, String)]) -> BTreeMap<String, String> {
    let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, version) in packages {
        let entry = versions.entry(name.clone()).or_default();
        if entry.last() != Some(version) {
            entry.push(version.clone());
        }
    }
    versions
        .into_iter()
        .map(|(name, versions)| (name, versions.join(",")))
        .collect()
}

fn attribution(tree: &str, direct_names: &HashSet<String>) -> HashMap<String, String> {
    let mut sources: HashMap<String, Vec<String>> = HashMap::new();
    let mut ancestors: Vec<(usize, &str)> = Vec::new();

    for line in tree.lines() {
        let Some((depth, name)) = tree_entry(line) else {
            continue;
        };
        while ancestors.last().is_some_and(|(level, _)| *level >= depth) {
            ancestors.pop();
        }
        if depth == 0 {
            continue;
        }
        if direct_names.contains(name) {
            ancestors.push((depth, name));
            continue;
        }
        let Some(&(_, direct)) = ancestors.last() else {
            continue;
        };
        let entry = sources.entry(name.to_string()).or_default();
        if !entry.iter().any(|seen| seen == direct) {
            entry.push(direct.to_string());
        }
    }

    sources
        .into_iter()
        .map(|(name, sources)| (name, truncate_sources(&sources)))
        .collect()
}

fn tree_entry(line: &str) -> Option<(usize, &str)> {
    let digit_count = line.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 || digit_count == line.len() {
        return None;
    }
    let depth = line[..digit_count].parse().ok()?;
    let rest = &line[digit_count..];
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if name_end == 0 {
        return None;
    }
    Some((depth, &rest[..name_end]))
}

fn truncate_sources(sources: &[String]) -> String {
    if sources.len() <= 3 {
        sources.join(",")
    } else {
        format!(
            "{},{},{},...(+{})",
            sources[0],
            sources[1],
            sources[2],
            sources.len() - 3
        )
    }
}

#[cfg(test)]
#[path = "lock/tests.rs"]
mod tests;
