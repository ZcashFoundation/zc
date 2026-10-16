//! Which Cargo features zc enables when it documents each crate.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use crate::cargo_meta::{self, FeatureTables};

/// The feature policy selected with `--features`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeaturePolicy {
    /// Every feature, so no feature-gated public item is missing from the diff.
    All,
    /// Each crate's default features.
    Default,
    /// Every feature a crate declares except these names, and except any feature of that
    /// crate that enables one of them.
    Exclude(BTreeSet<String>),
}

impl FeaturePolicy {
    /// Parses `all`, `default`, or `exclude:<feature>[,<feature>...]`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "default" => Some(Self::Default),
            _ => {
                let excluded: BTreeSet<String> = value
                    .strip_prefix("exclude:")?
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
                (!excluded.is_empty()).then_some(Self::Exclude(excluded))
            }
        }
    }

    /// Lines folded into the rustdoc JSON cache fingerprint. `all` keeps the value used
    /// before policies existed, so caches written by earlier releases stay valid.
    pub fn fingerprint_args(&self) -> Vec<String> {
        match self {
            Self::All => vec!["--all-features".to_owned()],
            Self::Default => Vec::new(),
            Self::Exclude(_) => vec![self.to_string()],
        }
    }
}

impl fmt::Display for FeaturePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("all"),
            Self::Default => f.write_str("default"),
            Self::Exclude(names) => {
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                write!(f, "exclude:{}", names.join(","))
            }
        }
    }
}

/// Cargo arguments that apply `policy` to a crate whose `[features]` table is `declared`.
pub fn cargo_args(policy: &FeaturePolicy, declared: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    match policy {
        FeaturePolicy::All => vec!["--all-features".to_owned()],
        FeaturePolicy::Default => Vec::new(),
        FeaturePolicy::Exclude(excluded) => {
            let kept = kept_features(declared, excluded);
            let mut args = vec!["--no-default-features".to_owned()];
            if !kept.is_empty() {
                args.push("--features".to_owned());
                args.push(kept.join(","));
            }
            args
        }
    }
}

/// Declared features that neither are excluded nor enable an excluded feature of the same
/// crate, directly or through other features.
fn kept_features(
    declared: &BTreeMap<String, Vec<String>>,
    excluded: &BTreeSet<String>,
) -> Vec<String> {
    declared
        .keys()
        .filter(|feature| !enables_excluded(feature, declared, excluded, &mut BTreeSet::new()))
        .cloned()
        .collect()
}

fn enables_excluded(
    feature: &str,
    declared: &BTreeMap<String, Vec<String>>,
    excluded: &BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if excluded.contains(feature) {
        return true;
    }
    if !visited.insert(feature.to_owned()) {
        return false;
    }
    let Some(entries) = declared.get(feature) else {
        return false;
    };
    entries.iter().any(|entry| {
        // `dep:x` enables only an optional dependency, and `x?/f` leaves `x` itself off.
        // `x/f` also enables `x`'s implicit feature, and a bare name is another feature.
        let enabled = match entry.split_once('/') {
            _ if entry.starts_with("dep:") => None,
            Some((dep, _)) if dep.ends_with('?') => None,
            Some((dep, _)) => Some(dep),
            None => Some(entry.as_str()),
        };
        enabled.is_some_and(|name| enables_excluded(name, declared, excluded, visited))
    })
}

/// Resolves each crate's feature arguments per ref, reading each ref's `[features]`
/// tables once. A ref can declare different features from the other one.
pub struct CrateFeatures {
    policy: FeaturePolicy,
    tables_by_ref: Mutex<HashMap<String, FeatureTables>>,
}

impl CrateFeatures {
    pub fn new(policy: FeaturePolicy) -> Self {
        Self {
            policy,
            tables_by_ref: Mutex::new(HashMap::new()),
        }
    }

    pub fn policy(&self) -> &FeaturePolicy {
        &self.policy
    }

    /// Cargo feature arguments for `crate_name` in `worktree`, a checkout of `ref_sha`.
    pub fn cargo_args(
        &self,
        crate_name: &str,
        worktree: &Path,
        ref_sha: &str,
    ) -> Result<Vec<String>, String> {
        if !matches!(self.policy, FeaturePolicy::Exclude(_)) {
            return Ok(cargo_args(&self.policy, &BTreeMap::new()));
        }
        let mut tables_by_ref = self.lock();
        let tables = Self::tables(&mut tables_by_ref, worktree, ref_sha)?;
        let declared = tables.get(crate_name).cloned().unwrap_or_default();
        Ok(cargo_args(&self.policy, &declared))
    }

    /// Excluded names that no workspace crate in `worktree` declares, which usually means
    /// a typo: the excluded surface would still be analysed.
    pub fn undeclared_exclusions(&self, worktree: &Path, ref_sha: &str) -> Vec<String> {
        let FeaturePolicy::Exclude(excluded) = &self.policy else {
            return Vec::new();
        };
        let mut tables_by_ref = self.lock();
        let Ok(tables) = Self::tables(&mut tables_by_ref, worktree, ref_sha) else {
            return Vec::new();
        };
        excluded
            .iter()
            .filter(|name| !tables.values().any(|declared| declared.contains_key(*name)))
            .cloned()
            .collect()
    }

    fn tables<'a>(
        tables_by_ref: &'a mut HashMap<String, FeatureTables>,
        worktree: &Path,
        ref_sha: &str,
    ) -> Result<&'a FeatureTables, String> {
        if !tables_by_ref.contains_key(ref_sha) {
            let tables = cargo_meta::workspace_feature_tables(worktree)?;
            tables_by_ref.insert(ref_sha.to_owned(), tables);
        }
        Ok(&tables_by_ref[ref_sha])
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, FeatureTables>> {
        self.tables_by_ref
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests;
