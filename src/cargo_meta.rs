use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

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
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    rename: Option<String>,
    req: Option<String>,
    kind: Option<String>,
    optional: Option<bool>,
    uses_default_features: Option<bool>,
    features: Option<Vec<String>>,
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
}

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

/// Lists the names of workspace-member crates in sorted order.
pub fn workspace_crate_names(workspace_dir: &Path) -> Vec<String> {
    let manifest = workspace_dir.join("Cargo.toml");
    let Ok(output) = Command::new("cargo")
        .arg("metadata")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(metadata) = serde_json::from_slice::<Metadata>(&output.stdout) else {
        return Vec::new();
    };

    let members: HashSet<_> = metadata.workspace_members.into_iter().collect();
    let mut names: Vec<_> = metadata
        .packages
        .into_iter()
        .filter(|package| members.contains(&package.id))
        .map(|package| package.name)
        .collect();
    names.sort();
    names
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

    let output = locked_metadata(&worktree);
    git::worktree_remove(&worktree);
    let output = match output {
        Ok(output) if output.status.success() => output,
        _ => {
            let message =
                format!("lockfile out of sync at '{git_ref}' (cargo metadata --locked failed)");
            eprintln!("{}error:{} {message}", ctx.style.red, ctx.style.reset);
            eprintln!(
                "{}  either update Cargo.lock at that ref, or re-run without --locked by editing this script{}",
                ctx.style.dim, ctx.style.reset
            );
            return Err(message);
        }
    };

    let metadata = match serde_json::from_slice::<Metadata>(&output.stdout) {
        Ok(metadata) => metadata,
        Err(_) => {
            let message = format!("jq failed processing metadata for '{git_ref}'");
            eprintln!("{}error:{} {message}", ctx.style.red, ctx.style.reset);
            return Err(message);
        }
    };
    let records = classify_workspace_deps(metadata);
    if let Ok(contents) = encode_workspace_cache(&records) {
        ctx.cache.write_atomic(&cache_name, &contents);
    }
    Ok(records)
}

/// Reads direct runtime and build dependencies for each workspace crate at a git ref.
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

    let output = locked_metadata(&worktree);
    git::worktree_remove(&worktree);
    let output = match output {
        Ok(output) if output.status.success() => output,
        _ => {
            let message = format!("cargo metadata failed at '{git_ref}' (per-crate deps)");
            eprintln!("{}warning:{} {message}", ctx.style.yellow, ctx.style.reset);
            return Err(message);
        }
    };

    let metadata = match serde_json::from_slice::<Metadata>(&output.stdout) {
        Ok(metadata) => metadata,
        Err(_) => {
            let message = format!("jq failed processing per-crate deps for '{git_ref}'");
            eprintln!("{}warning:{} {message}", ctx.style.yellow, ctx.style.reset);
            return Err(message);
        }
    };
    let records = classify_per_crate_deps(metadata);
    if let Ok(contents) = encode_per_crate_cache(&records) {
        ctx.cache.write_atomic(&cache_name, &contents);
    }
    Ok(records)
}

fn locked_metadata(worktree: &Path) -> std::io::Result<std::process::Output> {
    Command::new("cargo")
        .arg("metadata")
        .arg("--manifest-path")
        .arg(worktree.join("Cargo.toml"))
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .arg("--locked")
        .stderr(Stdio::null())
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
    let mut declared: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut usages: BTreeMap<String, Aggregate> = BTreeMap::new();

    for package in metadata.packages {
        if is_test_crate(&package.name) {
            continue;
        }
        for dependency in package.dependencies {
            let key = dependency
                .rename
                .clone()
                .unwrap_or_else(|| dependency.name.clone());
            declared.entry(key.clone()).or_insert_with(|| {
                (
                    dependency.name.clone(),
                    strip_caret(dependency.req.as_deref().unwrap_or("-")),
                )
            });
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
        .filter(|(_, (real_name, _))| !workspace_names.contains(real_name))
        .map(|(key, (real_name, ver))| {
            let aggregate = usages.remove(&key).unwrap_or_else(|| Aggregate {
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
                    ver,
                    kind: aggregate.kind,
                    optional: aggregate.optional,
                    default_features: aggregate.uses_default,
                    features: aggregate.features.into_iter().collect(),
                },
            )
        })
        .collect()
}

fn merge_usage(usages: &mut BTreeMap<String, Aggregate>, key: String, usage: Usage) {
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
    let mut records = BTreeMap::new();

    for package in &packages {
        records
            .entry((package.name.clone(), "~msrv".to_string()))
            .or_insert_with(|| PerCrateDep {
                crate_name: package.name.clone(),
                dep: "~msrv".to_string(),
                req: package
                    .rust_version
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                scope: Scope::Msrv,
                pkg: "-".to_string(),
            });
    }
    for package in packages {
        for dependency in package.dependencies {
            if !matches!(
                dependency.kind.as_deref(),
                None | Some("normal") | Some("build")
            ) {
                continue;
            }
            let dep = dependency
                .rename
                .clone()
                .unwrap_or_else(|| dependency.name.clone());
            let key = (package.name.clone(), dep.clone());
            records.entry(key).or_insert_with(|| PerCrateDep {
                crate_name: package.name.clone(),
                dep,
                req: strip_caret(dependency.req.as_deref().unwrap_or("-")),
                scope: if workspace_names.contains(&dependency.name) {
                    Scope::Internal
                } else {
                    Scope::External
                },
                pkg: dependency.name,
            });
        }
    }
    records.into_values().collect()
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
    serde_json::to_string(&cached)
}

fn decode_workspace_cache(contents: &str) -> Option<BTreeMap<String, DepRecord>> {
    let cached: Vec<CachedDep> = serde_json::from_str(contents).ok()?;
    cached
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
        })
        .collect();
    serde_json::to_string(&cached)
}

fn decode_per_crate_cache(contents: &str) -> Option<Vec<PerCrateDep>> {
    let cached: Vec<CachedPerCrateDep> = serde_json::from_str(contents).ok()?;
    cached
        .into_iter()
        .map(|record| {
            let scope = match record.scope.as_str() {
                "int" => Scope::Internal,
                "ext" => Scope::External,
                "msrv" => Scope::Msrv,
                _ => return None,
            };
            Some(PerCrateDep {
                crate_name: record.crate_name,
                dep: record.dep,
                req: record.req,
                scope,
                pkg: record.pkg,
            })
        })
        .collect()
}
