//! Reachable public dependencies whose package identity or requirements changed incompatibly.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::ctx::Ctx;
use crate::model::{Bump, DepKind, PerCrateDep, PubdepFinding, Scope};
use crate::version_req::classify_bump;

/// Runtime dependency rows, retaining distinct target-specific requirements.
pub struct PubdepTables {
    base: HashMap<String, Vec<PerCrateDep>>,
    head: HashMap<String, Vec<PerCrateDep>>,
}

impl PubdepTables {
    pub fn build(base: &[PerCrateDep], head: &[PerCrateDep]) -> PubdepTables {
        fn collect(rows: &[PerCrateDep]) -> HashMap<String, Vec<PerCrateDep>> {
            let mut table: HashMap<String, Vec<PerCrateDep>> = HashMap::new();
            for row in rows
                .iter()
                .filter(|row| row.scope == Scope::External && row.kind == DepKind::Runtime)
            {
                table
                    .entry(row.crate_name.clone())
                    .or_default()
                    .push(row.clone());
            }
            table
        }
        PubdepTables {
            base: collect(base),
            head: collect(head),
        }
    }

    fn candidates(&self, crate_name: &str) -> Vec<(&PerCrateDep, &PerCrateDep)> {
        let (Some(base), Some(head)) = (self.base.get(crate_name), self.head.get(crate_name))
        else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        for new in head {
            let exact = |old: &&PerCrateDep| old.dep == new.dep && old.target == new.target;
            if base.iter().any(|old| exact(&old)) {
                candidates.extend(base.iter().filter(exact).map(|old| (old, new)));
            } else {
                candidates.extend(
                    base.iter()
                        .filter(|old| {
                            old.pkg == new.pkg
                                && old.target == new.target
                                && !head
                                    .iter()
                                    .any(|new| old.dep == new.dep && old.target == new.target)
                        })
                        .map(|old| (old, new)),
                );
            }
        }
        // Keep unchanged alternatives too: they can make a changed pairing ambiguous.
        candidates
    }
}

fn needs_comparison(old: &PerCrateDep, new: &PerCrateDep) -> bool {
    old.pkg != new.pkg
        || (old.req != new.req
            && matches!(
                classify_bump(&old.req, &new.req),
                Bump::Major | Bump::Unknown
            ))
}

/// Joins changed runtime packages/requirements with actual baseline and head public provenance.
pub fn compute_from_json(
    ctx: &Ctx,
    tables: &PubdepTables,
    crate_name: &str,
    base_json: Option<&Path>,
    head_json: &Path,
) -> Result<Vec<PubdepFinding>, String> {
    let candidates = tables.candidates(crate_name);
    if !candidates
        .iter()
        .any(|(old, new)| needs_comparison(old, new))
    {
        return Ok(Vec::new());
    }
    let Some(base_json) = base_json else {
        return Ok(Vec::new());
    };
    let base_foreign = read_foreign_crates(base_json)?;
    if base_foreign.is_empty() {
        return Ok(Vec::new());
    }
    let head_foreign = read_foreign_crates(head_json)?;
    if head_foreign.is_empty() {
        return Ok(Vec::new());
    }
    let base_metadata = dependency_metadata(ctx, crate_name, true)?;
    let head_metadata = dependency_metadata(ctx, crate_name, false)?;
    findings(
        &candidates,
        Some((&base_foreign, &base_metadata)),
        (&head_foreign, &head_metadata),
        crate_name,
    )
}

fn read_foreign_crates(path: &Path) -> Result<HashSet<String>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("cannot read public dependency rustdoc JSON: {error}"))?;
    foreign_crates(&crate::rustdoc::parse_json(&contents)?)
}

fn dependency_metadata(ctx: &Ctx, crate_name: &str, baseline: bool) -> Result<Value, String> {
    let (worktree, sha) = if baseline {
        (&ctx.baseline_worktree, &ctx.refs.baseline_sha)
    } else {
        (&ctx.head_worktree, &ctx.refs.head_sha)
    };
    let mut feature_args = ctx.features.cargo_args(crate_name, worktree, sha)?;
    // `metadata` selects the workspace, unlike `rustdoc -p`; qualify explicit features.
    if let Some(index) = feature_args.iter().position(|arg| arg == "--features") {
        feature_args[index + 1] = feature_args[index + 1]
            .split(',')
            .map(|feature| format!("{crate_name}/{feature}"))
            .collect::<Vec<_>>()
            .join(",");
    }
    let output = Command::new("cargo")
        .current_dir(worktree)
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(worktree.join("Cargo.toml"))
        .args(feature_args)
        .output()
        .map_err(|error| format!("public dependency cargo metadata failed to start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "public dependency cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("unreadable public dependency cargo metadata: {error}"))
}

/// Reads crate IDs only from actual public items and the type/item IDs in their signatures.
fn foreign_crates(root: &Value) -> Result<HashSet<String>, String> {
    let index = root["index"]
        .as_object()
        .ok_or("rustdoc JSON has no item index")?;
    let paths = root["paths"]
        .as_object()
        .ok_or("rustdoc JSON has no path index")?;
    let external = root["external_crates"]
        .as_object()
        .ok_or("rustdoc JSON has no external crate index")?;
    let root_id = crate::rustdoc::id_text(&root["root"]).ok_or("rustdoc JSON has no root ID")?;
    let local = index
        .get(&root_id)
        .and_then(|item| item["crate_id"].as_u64())
        .ok_or("rustdoc JSON has no root crate ID")?;
    fn references(
        value: &Value,
        index: &serde_json::Map<String, Value>,
        paths: &serde_json::Map<String, Value>,
        crates: &mut HashSet<u64>,
    ) -> Result<(), String> {
        match value {
            Value::Array(values) => {
                for value in values {
                    references(value, index, paths, crates)?;
                }
            }
            Value::Object(values) => {
                if let Some(item_id) = values.get("id").and_then(crate::rustdoc::id_text) {
                    let crate_id = index
                        .get(&item_id)
                        .or_else(|| paths.get(&item_id))
                        .and_then(|item| item["crate_id"].as_u64())
                        .ok_or_else(|| {
                            format!("rustdoc JSON has no provenance for item {item_id}")
                        })?;
                    crates.insert(crate_id);
                }
                for value in values.values() {
                    references(value, index, paths, crates)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut crates = HashSet::new();
    let public = crate::rustdoc::public_items(root).items;
    if public.is_empty() {
        return Err("rustdoc JSON has no public root".into());
    }
    for item in public.values() {
        if let Some(crate_id) = item["crate_id"].as_u64() {
            crates.insert(crate_id);
        }
        // Documentation links and arbitrary text are not public type provenance.
        references(&item["inner"], index, paths, &mut crates)?;
    }
    crates.remove(&local);
    crates
        .into_iter()
        .map(|crate_id| {
            external
                .get(&crate_id.to_string())
                .and_then(|item| item["name"].as_str())
                .map(str::to_owned)
                .ok_or_else(|| format!("rustdoc JSON has no provenance for crate {crate_id}"))
        })
        .collect()
}

fn library_target(target: &Value) -> bool {
    target["kind"].as_array().is_some_and(|kinds| {
        kinds.iter().any(|kind| {
            matches!(
                kind.as_str(),
                Some("lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro")
            )
        })
    })
}

struct PublicDependencies<'a> {
    packages: &'a [Value],
    deps: &'a [Value],
    foreign: &'a HashSet<String>,
    ambiguous: HashSet<&'a str>,
}

fn public_dependencies<'a>(
    foreign: &'a HashSet<String>,
    metadata: &'a Value,
    crate_name: &str,
) -> Result<PublicDependencies<'a>, String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;
    let members = metadata["workspace_members"]
        .as_array()
        .ok_or("cargo metadata has no workspace members")?;
    let package = packages
        .iter()
        .find(|package| package["name"] == crate_name && members.contains(&package["id"]))
        .ok_or_else(|| format!("cargo metadata has no workspace package {crate_name}"))?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata has no resolved dependency graph")?;
    let node = nodes
        .iter()
        .find(|node| node["id"] == package["id"])
        .ok_or_else(|| format!("cargo metadata has no resolved package {crate_name}"))?;
    let deps = node["deps"]
        .as_array()
        .ok_or("cargo metadata has no resolved dependencies")?;
    let mut runtime = HashSet::new();
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        let deps = node["deps"]
            .as_array()
            .ok_or("cargo metadata has no resolved dependencies")?;
        for dep in deps {
            let kinds = dep["dep_kinds"]
                .as_array()
                .ok_or("cargo metadata has no resolved dependency kinds")?;
            if !kinds
                .iter()
                .any(|kind| kind["kind"].is_null() || kind["kind"] == "normal")
            {
                continue;
            }
            let package_id = dep["pkg"]
                .as_str()
                .ok_or("cargo metadata has no dependency package ID")?;
            if runtime.insert(package_id) {
                let dependency = nodes
                    .iter()
                    .find(|node| node["id"] == package_id)
                    .ok_or_else(|| {
                        format!("cargo metadata has no resolved package {package_id}")
                    })?;
                pending.push(dependency);
            }
        }
    }
    // Rustdoc identifies external crates by library name, not Cargo package ID. Never guess
    // which version/package supplied a foreign item when that name is ambiguous.
    let mut library_owners = HashMap::new();
    let mut ambiguous = HashSet::new();
    for package in packages {
        if !package["id"]
            .as_str()
            .is_some_and(|id| runtime.contains(id))
        {
            continue;
        }
        if let Some(targets) = package["targets"].as_array() {
            for target in targets.iter().filter(|target| library_target(target)) {
                if let Some(name) = target["name"]
                    .as_str()
                    .filter(|name| foreign.contains(*name))
                {
                    if let Some(previous) = library_owners.insert(name, &package["id"]) {
                        if previous != &package["id"] {
                            ambiguous.insert(name);
                        }
                    }
                }
            }
        }
    }
    Ok(PublicDependencies {
        packages,
        deps,
        foreign,
        ambiguous,
    })
}

fn is_public(row: &PerCrateDep, resolved: &PublicDependencies<'_>) -> Result<bool, String> {
    let PublicDependencies {
        packages,
        deps,
        foreign,
        ambiguous,
    } = resolved;
    let alias = row.dep.replace('-', "_");
    let mut reachable = false;
    for dep in *deps {
        let kinds = dep["dep_kinds"]
            .as_array()
            .ok_or("cargo metadata has no resolved dependency kinds")?;
        if !kinds.iter().any(|kind| {
            (kind["kind"].is_null() || kind["kind"] == "normal")
                && kind["target"].as_str() == row.target.as_deref()
        }) {
            continue;
        }
        let package = packages
            .iter()
            .find(|package| package["id"] == dep["pkg"])
            .ok_or_else(|| format!("cargo metadata has no resolved dependency {}", row.dep))?;
        // Unrenamed dependencies resolve by their library target, not the package name.
        if package["name"] != row.pkg || (dep["name"] != alias && row.dep != row.pkg) {
            continue;
        }
        let targets = package["targets"]
            .as_array()
            .ok_or("cargo metadata dependency has no targets")?;
        for target in targets.iter().filter(|target| library_target(target)) {
            if let Some(name) = target["name"]
                .as_str()
                .filter(|name| foreign.contains(*name))
            {
                if ambiguous.contains(name) {
                    return Err(format!(
                        "cannot identify Cargo package for public foreign library {name}: \
                         multiple resolved packages use that library name"
                    ));
                }
                reachable = true;
            }
        }
    }
    Ok(reachable)
}

fn findings(
    candidates: &[(&PerCrateDep, &PerCrateDep)],
    base: Option<(&HashSet<String>, &Value)>,
    head: (&HashSet<String>, &Value),
    crate_name: &str,
) -> Result<Vec<PubdepFinding>, String> {
    let Some((base_foreign, base_metadata)) = base else {
        return Ok(Vec::new());
    };
    let changed: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|(old, new)| needs_comparison(old, new))
        .collect();
    if changed.is_empty() || base_foreign.is_empty() || head.0.is_empty() {
        return Ok(Vec::new());
    }
    let base = public_dependencies(base_foreign, base_metadata, crate_name)?;
    let head = public_dependencies(head.0, head.1, crate_name)?;
    let mut exposed = Vec::new();
    for &(old, new) in candidates {
        if !changed.iter().any(|&(changed_old, changed_new)| {
            std::ptr::eq(old, changed_old) || std::ptr::eq(new, changed_new)
        }) {
            continue;
        }
        if is_public(old, &base)? && is_public(new, &head)? {
            exposed.push((old, new));
        }
    }
    let mut result = Vec::new();
    for (old, new) in &changed {
        if !exposed.iter().any(|(exposed_old, exposed_new)| {
            std::ptr::eq(*old, *exposed_old) && std::ptr::eq(*new, *exposed_new)
        }) {
            continue;
        }
        // ponytail: scan exposed links; index declarations if large migrations make this hot.
        if exposed.iter().any(|(other_old, other_new)| {
            (std::ptr::eq(*old, *other_old) && !std::ptr::eq(*new, *other_new))
                || (std::ptr::eq(*new, *other_new) && !std::ptr::eq(*old, *other_old))
        }) {
            return Err(format!(
                "cannot identify previous public dependency declaration for {}: \
                     ambiguous alias correspondence for package {}",
                new.dep, new.pkg
            ));
        }
        result.push(PubdepFinding {
            dep: new.dep.clone(),
            old_package: old.pkg.clone(),
            new_package: new.pkg.clone(),
            old: old.req.clone(),
            new: new.req.clone(),
            target: new.target.clone(),
        });
    }
    Ok(result)
}

#[cfg(test)]
#[path = "pubdep/tests.rs"]
mod tests;
