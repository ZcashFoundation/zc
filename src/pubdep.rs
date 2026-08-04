//! Reachable public dependencies whose version requirements changed incompatibly.

use std::collections::HashMap;
use std::process::{Command, Stdio};

use regex::Regex;

use crate::ctx::Ctx;
use crate::model::{Bump, PerCrateDep, PubdepFinding, Scope};
use crate::version_req::classify_bump;

/// Per-crate dependency requirements and head-side external dependency order.
pub struct PubdepTables {
    base: HashMap<String, HashMap<String, String>>,
    head: HashMap<String, HashMap<String, String>>,
    packages: HashMap<String, HashMap<String, String>>,
    external: HashMap<String, Vec<String>>,
}

impl PubdepTables {
    pub fn build(base: &[PerCrateDep], head: &[PerCrateDep]) -> PubdepTables {
        let mut tables = PubdepTables {
            base: HashMap::new(),
            head: HashMap::new(),
            packages: HashMap::new(),
            external: HashMap::new(),
        };
        for row in base.iter().filter(|row| row.scope != Scope::Msrv) {
            tables
                .base
                .entry(row.crate_name.clone())
                .or_default()
                .insert(row.dep.clone(), row.req.clone());
        }
        for row in head.iter().filter(|row| row.scope != Scope::Msrv) {
            tables
                .head
                .entry(row.crate_name.clone())
                .or_default()
                .insert(row.dep.clone(), row.req.clone());
            tables
                .packages
                .entry(row.crate_name.clone())
                .or_default()
                .insert(row.dep.clone(), row.pkg.clone());
            if row.scope == Scope::External {
                tables
                    .external
                    .entry(row.crate_name.clone())
                    .or_default()
                    .push(row.dep.clone());
            }
        }
        tables
    }

    pub fn req(&self, head_side: bool, crate_name: &str, dep: &str) -> Option<&str> {
        let table = if head_side { &self.head } else { &self.base };
        table
            .get(crate_name)
            .and_then(|deps| deps.get(dep))
            .map(String::as_str)
    }
}

/// Finds incompatible external dependencies that occur in a crate's full head API.
pub fn compute(ctx: &Ctx, tables: &PubdepTables, crate_name: &str) -> Vec<PubdepFinding> {
    let Some(external) = tables.external.get(crate_name) else {
        return Vec::new();
    };
    let candidates: Vec<_> = external
        .iter()
        .filter_map(|dep| {
            let old = tables.req(false, crate_name, dep)?;
            let new = tables.req(true, crate_name, dep)?;
            if old.is_empty()
                || new.is_empty()
                || old == new
                || !matches!(classify_bump(old, new), Bump::Major | Bump::Unknown)
            {
                return None;
            }
            Some((dep.as_str(), old, new))
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut command = Command::new("cargo");
    command
        .env("CARGO_TARGET_DIR", &ctx.head_target)
        .arg(format!("+{}", ctx.toolchain))
        .arg("public-api")
        .args(&ctx.feature_args)
        .arg("--manifest-path")
        .arg(ctx.head_worktree.join("Cargo.toml"))
        .arg("-p")
        .arg(crate_name)
        .arg("-ss")
        .stderr(Stdio::null());
    let Ok(output) = command.output() else {
        return Vec::new();
    };
    if output.stdout.is_empty() {
        return Vec::new();
    }
    let surface = String::from_utf8_lossy(&output.stdout);

    candidates
        .into_iter()
        .filter(|(dep, _, _)| {
            let pkg = tables
                .packages
                .get(crate_name)
                .and_then(|packages| packages.get(*dep))
                .filter(|pkg| !pkg.is_empty())
                .map(String::as_str)
                .unwrap_or(dep);
            reachable(&surface, dep, pkg)
        })
        .map(|(dep, old, new)| PubdepFinding {
            dep: dep.to_string(),
            old: old.to_string(),
            new: new.to_string(),
        })
        .collect()
}

fn reachable(surface: &str, dep: &str, pkg: &str) -> bool {
    let dep = dep.replace('-', "_");
    let pkg = pkg.replace('-', "_");
    let pattern = format!(
        r"(^|[^A-Za-z0-9_])({}|{})::",
        regex::escape(&dep),
        regex::escape(&pkg)
    );
    Regex::new(&pattern).is_ok_and(|regex| regex.is_match(surface))
}

#[cfg(test)]
#[path = "pubdep/tests.rs"]
mod tests;
