use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use crate::git;
use crate::model::{is_test_crate, DepKind, DepRecord, PerCrateDep, Scope};

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    packages: Vec<Package>,
    #[serde(default)]
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    rust_version: Option<String>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

/// Cargo's actual library target name, absent for binary-only members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCrate {
    pub lib_name: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CrateFeatureTable {
    pub features: BTreeMap<String, Vec<String>>,
    pub optional_dependencies: BTreeSet<String>,
}

/// Each workspace crate's feature graph and optional dependencies, by package name.
pub type FeatureTables = BTreeMap<String, CrateFeatureTable>;

#[derive(Deserialize)]
struct Dependency {
    name: String,
    rename: Option<String>,
    req: Option<String>,
    kind: Option<String>,
    optional: Option<bool>,
    uses_default_features: Option<bool>,
    features: Option<Vec<String>>,
    target: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CachedDep {
    key: String,
    ver: String,
    kind: String,
    optional: bool,
    default_features: bool,
    features: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct CachedPerCrateDep {
    crate_name: String,
    dep: String,
    req: String,
    scope: String,
    pkg: String,
    target: Option<String>,
    kind: String,
    optional: bool,
}

#[derive(Serialize, Deserialize)]
struct CachedRows<T> {
    schema: u32,
    rows: Vec<T>,
}

const CACHE_SCHEMA: u32 = 3;

struct Usage {
    kind: DepKind,
    optional: bool,
    uses_default: bool,
    features: Vec<String>,
}

struct Aggregate {
    kind: DepKind,
    optional: bool,
    uses_default: bool,
    features: BTreeSet<String>,
}

/// Lists all workspace members, including private and binary-only packages.
pub fn workspace_crates(workspace_dir: &Path) -> Result<BTreeMap<String, WorkspaceCrate>, String> {
    let metadata = read_metadata(workspace_dir)?;
    Ok(classify_workspace_crates(metadata))
}

fn classify_workspace_crates(metadata: Metadata) -> BTreeMap<String, WorkspaceCrate> {
    let members: HashSet<_> = metadata.workspace_members.into_iter().collect();
    metadata
        .packages
        .into_iter()
        .filter(|package| members.contains(&package.id))
        .map(|package| {
            let lib_name = package.targets.into_iter().find_map(|target| {
                target
                    .kind
                    .iter()
                    .any(|kind| {
                        matches!(
                            kind.as_str(),
                            "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
                        )
                    })
                    .then_some(target.name)
            });
            (package.name, WorkspaceCrate { lib_name })
        })
        .collect()
}

/// Lists workspace-member package names in sorted order.
#[allow(dead_code)]
pub fn workspace_crate_names(workspace_dir: &Path) -> Result<Vec<String>, String> {
    Ok(workspace_crates(workspace_dir)?.into_keys().collect())
}

/// Reads every workspace member's feature graph and dependency optionality.
pub fn workspace_feature_tables(workspace_dir: &Path) -> Result<FeatureTables, String> {
    let metadata = read_metadata(workspace_dir)?;
    Ok(classify_feature_tables(metadata))
}

fn classify_feature_tables(metadata: Metadata) -> FeatureTables {
    let members: HashSet<_> = metadata.workspace_members.into_iter().collect();
    metadata
        .packages
        .into_iter()
        .filter(|package| members.contains(&package.id))
        .map(|package| {
            let optional_dependencies = package
                .dependencies
                .into_iter()
                .filter(|dependency| dependency.optional.unwrap_or(false))
                .map(|dependency| dependency.rename.unwrap_or(dependency.name))
                .collect();
            (
                package.name,
                CrateFeatureTable {
                    features: package.features,
                    optional_dependencies,
                },
            )
        })
        .collect()
}

fn read_metadata(worktree: &Path) -> Result<Metadata, String> {
    let output = locked_metadata(worktree)
        .map_err(|error| format!("cargo metadata failed to start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata --locked failed in {}: {}",
            worktree.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata returned unreadable JSON: {error}"))
}

/// Reads the merged external dependency table at a git ref.
pub fn dump_workspace_deps(
    ctx: &Ctx,
    git_ref: &str,
) -> Result<BTreeMap<String, DepRecord>, String> {
    let sha =
        git::rev_parse_verify(git_ref).map_err(|_| format!("cannot resolve ref '{git_ref}'"))?;
    let cache_name = format!("{sha}.{}.tsv", ctx.cache.script_hash);
    if let Some(contents) = ctx.cache.read_if_present(&cache_name) {
        if let Some(cached) = decode_workspace_cache(&contents) {
            return Ok(cached);
        }
    }

    let worktree = ctx.tmp.sub("deps")?;
    if git::worktree_add(&worktree, &sha).is_err() {
        let message = format!("failed to create worktree for '{git_ref}'");
        eprintln!("{}error:{} {message}", ctx.style.red, ctx.style.reset);
        return Err(message);
    }

    let metadata = read_metadata(&worktree);
    git::worktree_remove(&worktree);
    let metadata = metadata.map_err(|error| format!("metadata at '{git_ref}': {error}"))?;
    let records = classify_workspace_deps(metadata);
    if let Ok(contents) = encode_workspace_cache(&records) {
        ctx.cache.write_atomic(&cache_name, &contents);
    }
    Ok(records)
}

/// Reads every direct dependency row for each workspace crate at a git ref.
pub fn dump_per_crate_deps(ctx: &Ctx, git_ref: &str) -> Result<Vec<PerCrateDep>, String> {
    let sha =
        git::rev_parse_verify(git_ref).map_err(|_| format!("cannot resolve ref '{git_ref}'"))?;
    let cache_name = format!("{sha}.{}.percrate-deps.tsv", ctx.cache.script_hash);
    if let Some(contents) = ctx.cache.read_if_present(&cache_name) {
        if let Some(cached) = decode_per_crate_cache(&contents) {
            return Ok(cached);
        }
    }

    let worktree = ctx.tmp.sub("percrate-deps")?;
    if git::worktree_add(&worktree, &sha).is_err() {
        let message = format!("could not create worktree for '{git_ref}' (per-crate deps)");
        eprintln!("{}warning:{} {message}", ctx.style.yellow, ctx.style.reset);
        return Err(message);
    }

    let metadata = read_metadata(&worktree);
    git::worktree_remove(&worktree);
    let metadata = metadata.map_err(|error| format!("metadata at '{git_ref}': {error}"))?;
    let records = classify_per_crate_deps(metadata);
    if let Ok(contents) = encode_per_crate_cache(&records) {
        ctx.cache.write_atomic(&cache_name, &contents);
    }
    Ok(records)
}

fn locked_metadata(worktree: &Path) -> std::io::Result<std::process::Output> {
    Command::new("cargo")
        .current_dir(worktree)
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .arg("--locked")
        .output()
}

fn classify_workspace_deps(metadata: Metadata) -> BTreeMap<String, DepRecord> {
    let member_ids: HashSet<_> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let workspace_names: HashSet<_> = metadata
        .packages
        .iter()
        .filter(|package| member_ids.contains(package.id.as_str()))
        .map(|package| package.name.clone())
        .collect();
    let mut declared: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut usages = BTreeMap::new();

    for package in metadata.packages {
        if !member_ids.contains(package.id.as_str()) || is_test_crate(&package.name) {
            continue;
        }
        for dependency in package.dependencies {
            let alias = dependency
                .rename
                .clone()
                .unwrap_or_else(|| dependency.name.clone());
            let key = (alias, dependency.name.clone());
            declared
                .entry(key.clone())
                .or_default()
                .insert(strip_caret(dependency.req.as_deref().unwrap_or("-")));
            merge_usage(
                &mut usages,
                key,
                Usage {
                    kind: dependency_kind(dependency.kind.as_deref()),
                    optional: dependency.optional.unwrap_or(false),
                    uses_default: dependency.uses_default_features.unwrap_or(true),
                    features: dependency.features.unwrap_or_default(),
                },
            );
        }
    }

    declared
        .into_iter()
        .filter(|((_, real_name), _)| !workspace_names.contains(real_name))
        .map(|((key, real_name), requirements)| {
            let aggregate = usages
                .remove(&(key.clone(), real_name.clone()))
                .unwrap_or_else(|| Aggregate {
                    kind: DepKind::Unused,
                    optional: false,
                    uses_default: true,
                    features: BTreeSet::new(),
                });
            let display = if key == real_name {
                key
            } else {
                format!("{key} (pkg: {real_name})")
            };
            (
                display,
                DepRecord {
                    ver: requirements.into_iter().collect::<Vec<_>>().join(" | "),
                    kind: aggregate.kind,
                    optional: aggregate.optional,
                    default_features: aggregate.uses_default,
                    features: aggregate.features.into_iter().collect(),
                },
            )
        })
        .collect()
}

fn merge_usage(
    usages: &mut BTreeMap<(String, String), Aggregate>,
    key: (String, String),
    usage: Usage,
) {
    match usages.get_mut(&key) {
        Some(aggregate) => {
            if usage.kind.rank() > aggregate.kind.rank() {
                aggregate.kind = usage.kind;
                aggregate.optional = usage.optional;
            } else if usage.kind == aggregate.kind {
                aggregate.optional &= usage.optional;
            }
            aggregate.uses_default |= usage.uses_default;
            aggregate.features.extend(usage.features);
        }
        None => {
            usages.insert(
                key,
                Aggregate {
                    kind: usage.kind,
                    optional: usage.optional,
                    uses_default: usage.uses_default,
                    features: usage.features.into_iter().collect(),
                },
            );
        }
    }
}

fn classify_per_crate_deps(metadata: Metadata) -> Vec<PerCrateDep> {
    let member_ids: HashSet<_> = metadata.workspace_members.into_iter().collect();
    let workspace_names: HashSet<_> = metadata
        .packages
        .iter()
        .filter(|package| member_ids.contains(&package.id))
        .map(|package| package.name.clone())
        .collect();
    let packages: Vec<_> = metadata
        .packages
        .into_iter()
        .filter(|package| member_ids.contains(&package.id) && !is_test_crate(&package.name))
        .collect();
    let mut records = Vec::new();

    for package in &packages {
        records.push(PerCrateDep {
            crate_name: package.name.clone(),
            dep: "~msrv".to_string(),
            req: package
                .rust_version
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            scope: Scope::Msrv,
            pkg: "-".to_string(),
            target: None,
            kind: DepKind::Unused,
            optional: false,
        });
    }
    for package in packages {
        for dependency in package.dependencies {
            let dep = dependency
                .rename
                .clone()
                .unwrap_or_else(|| dependency.name.clone());
            records.push(PerCrateDep {
                crate_name: package.name.clone(),
                dep,
                req: strip_caret(dependency.req.as_deref().unwrap_or("-")),
                scope: if workspace_names.contains(&dependency.name) {
                    Scope::Internal
                } else {
                    Scope::External
                },
                pkg: dependency.name,
                target: dependency.target,
                kind: dependency_kind(dependency.kind.as_deref()),
                optional: dependency.optional.unwrap_or(false),
            });
        }
    }
    records.sort_by(|left, right| {
        (
            &left.crate_name,
            &left.dep,
            &left.target,
            left.kind.rank(),
            &left.req,
            &left.pkg,
        )
            .cmp(&(
                &right.crate_name,
                &right.dep,
                &right.target,
                right.kind.rank(),
                &right.req,
                &right.pkg,
            ))
    });
    records
}

fn dependency_kind(kind: Option<&str>) -> DepKind {
    match kind {
        None | Some("normal") => DepKind::Runtime,
        Some("build") => DepKind::Build,
        Some("dev") => DepKind::Dev,
        Some(_) => DepKind::Runtime,
    }
}

fn strip_caret(req: &str) -> String {
    req.strip_prefix('^').unwrap_or(req).to_string()
}

fn encode_workspace_cache(records: &BTreeMap<String, DepRecord>) -> serde_json::Result<String> {
    let cached: Vec<_> = records
        .iter()
        .map(|(key, record)| CachedDep {
            key: key.clone(),
            ver: record.ver.clone(),
            kind: record.kind.as_str().to_string(),
            optional: record.optional,
            default_features: record.default_features,
            features: record.features.clone(),
        })
        .collect();
    serde_json::to_string(&CachedRows {
        schema: CACHE_SCHEMA,
        rows: cached,
    })
}

fn decode_workspace_cache(contents: &str) -> Option<BTreeMap<String, DepRecord>> {
    let cached: CachedRows<CachedDep> = serde_json::from_str(contents).ok()?;
    if cached.schema != CACHE_SCHEMA {
        return None;
    }
    cached
        .rows
        .into_iter()
        .map(|record| {
            let kind = match record.kind.as_str() {
                "runtime" => DepKind::Runtime,
                "build" => DepKind::Build,
                "dev" => DepKind::Dev,
                "unused" => DepKind::Unused,
                _ => return None,
            };
            Some((
                record.key,
                DepRecord {
                    ver: record.ver,
                    kind,
                    optional: record.optional,
                    default_features: record.default_features,
                    features: record.features,
                },
            ))
        })
        .collect()
}

fn encode_per_crate_cache(records: &[PerCrateDep]) -> serde_json::Result<String> {
    let cached: Vec<_> = records
        .iter()
        .map(|record| CachedPerCrateDep {
            crate_name: record.crate_name.clone(),
            dep: record.dep.clone(),
            req: record.req.clone(),
            scope: match record.scope {
                Scope::Internal => "int",
                Scope::External => "ext",
                Scope::Msrv => "msrv",
            }
            .to_string(),
            pkg: record.pkg.clone(),
            target: record.target.clone(),
            kind: record.kind.as_str().to_string(),
            optional: record.optional,
        })
        .collect();
    serde_json::to_string(&CachedRows {
        schema: CACHE_SCHEMA,
        rows: cached,
    })
}

fn decode_per_crate_cache(contents: &str) -> Option<Vec<PerCrateDep>> {
    let cached: CachedRows<CachedPerCrateDep> = serde_json::from_str(contents).ok()?;
    if cached.schema != CACHE_SCHEMA {
        return None;
    }
    cached
        .rows
        .into_iter()
        .map(|record| {
            let scope = match record.scope.as_str() {
                "int" => Scope::Internal,
                "ext" => Scope::External,
                "msrv" => Scope::Msrv,
                _ => return None,
            };
            let kind = match record.kind.as_str() {
                "runtime" => DepKind::Runtime,
                "build" => DepKind::Build,
                "dev" => DepKind::Dev,
                "unused" => DepKind::Unused,
                _ => return None,
            };
            Some(PerCrateDep {
                crate_name: record.crate_name,
                dep: record.dep,
                req: record.req,
                scope,
                pkg: record.pkg,
                target: record.target,
                kind,
                optional: record.optional,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
