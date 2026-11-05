//! Workspace dependency classification.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    kind_label, label_rank, Bump, DepAdded, DepChanged, DepDiff, DepKind, DepRecord, DepRemoved,
    PerCrateDep, Scope,
};
use crate::version_req::classify_bump;

/// Render changes to default features and the explicit feature set.
pub fn feature_diff(old_def: bool, new_def: bool, old: &[String], new: &[String]) -> String {
    let mut delta = Vec::new();
    if old_def != new_def {
        delta.push(if old_def {
            "-default!".to_string()
        } else {
            "+default!".to_string()
        });
    }

    let old: BTreeSet<&str> = old
        .iter()
        .map(String::as_str)
        .filter(|f| !f.is_empty())
        .collect();
    let new: BTreeSet<&str> = new
        .iter()
        .map(String::as_str)
        .filter(|f| !f.is_empty())
        .collect();
    let mut features: Vec<String> = old
        .difference(&new)
        .map(|feature| format!("-{feature}"))
        .chain(new.difference(&old).map(|feature| format!("+{feature}")))
        .collect();
    features.sort();
    delta.extend(features);
    delta.join(",")
}

fn dependency_key(row: &PerCrateDep) -> (&str, &str, &str, Option<&str>, &str) {
    (
        &row.crate_name,
        &row.dep,
        &row.pkg,
        row.target.as_deref(),
        row.kind.as_str(),
    )
}

/// Compare matching dependency requirements before folding them into display rows.
/// Feature loss and whole-dependency additions/removals retain workspace-wide policy.
/// Only matching member/package/alias/target/kind rows establish version migrations.
/// Each row's head optionality decides whether its major migration is breaking.
pub fn diff(
    base: &BTreeMap<String, DepRecord>,
    head: &BTreeMap<String, DepRecord>,
    base_per_crate: &[PerCrateDep],
    head_per_crate: &[PerCrateDep],
) -> DepDiff {
    let mut result = DepDiff::default();
    let head_rows: BTreeMap<_, _> = head_per_crate
        .iter()
        .filter(|row| row.scope == Scope::External)
        .map(|row| (dependency_key(row), row))
        .collect();
    let mut requirement_changes = BTreeMap::new();
    for old in base_per_crate
        .iter()
        .filter(|row| row.scope == Scope::External)
    {
        let Some(new) = head_rows.get(&dependency_key(old)) else {
            continue;
        };
        if old.req == new.req {
            continue;
        }
        let bump = classify_bump(&old.req, &new.req);
        let breaking = bump == Bump::Major && new.kind == DepKind::Runtime && !new.optional;
        let change = requirement_changes
            .entry((old.dep.as_str(), old.pkg.as_str()))
            .or_insert((Bump::Unknown, false));
        change.0 = match (change.0, bump) {
            (Bump::Major, _) | (_, Bump::Major) => Bump::Major,
            (Bump::Minor, _) | (_, Bump::Minor) => Bump::Minor,
            (Bump::Patch, _) | (_, Bump::Patch) => Bump::Patch,
            _ => Bump::Unknown,
        };
        change.1 |= breaking;
    }

    for (name, old) in base {
        let Some(new) = head.get(name) else {
            let label = kind_label(old.kind, old.optional);
            let breaking = label == "runtime";
            if breaking {
                result.breaking += 1;
            }
            result.removed.push(DepRemoved {
                name: name.clone(),
                version: old.ver.clone(),
                kind: label,
                breaking,
            });
            continue;
        };

        let (alias, package) = name
            .split_once(" (pkg: ")
            .map(|(alias, package)| (alias, package.trim_end_matches(')')))
            .unwrap_or((name.as_str(), name.as_str()));
        let requirement_change = requirement_changes.get(&(alias, package));
        if old == new && requirement_change.is_none() {
            continue;
        }

        let (bump, major_breaking) = requirement_change
            .copied()
            .unwrap_or((Bump::Unknown, false));
        let old_label = kind_label(old.kind, old.optional);
        let new_label = kind_label(new.kind, new.optional);
        let label = if label_rank(&new_label) >= label_rank(&old_label) {
            new_label
        } else {
            old_label
        };
        let features = feature_diff(
            old.default_features,
            new.default_features,
            &old.features,
            &new.features,
        );

        let breaking = major_breaking
            || (label == "runtime" && features.split(',').any(|token| token.starts_with('-')));
        if breaking {
            result.breaking += 1;
        }

        result.changed.push(DepChanged {
            name: name.clone(),
            old: old.ver.clone(),
            new: new.ver.clone(),
            bump,
            kind: label,
            features,
            breaking,
        });
    }

    for (name, new) in head {
        if !base.contains_key(name) {
            result.added.push(DepAdded {
                name: name.clone(),
                version: new.ver.clone(),
                kind: kind_label(new.kind, new.optional),
            });
        }
    }

    result.removed.sort_by(|a, b| a.name.cmp(&b.name));
    result.changed.sort_by(|a, b| a.name.cmp(&b.name));
    result.added.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

#[cfg(test)]
#[path = "deps/tests.rs"]
mod tests;
