//! Workspace dependency classification.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    kind_label, label_rank, Bump, DepAdded, DepChanged, DepDiff, DepRecord, DepRemoved,
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

/// Compare workspace dependencies and classify consumer-visible breakage.
pub fn diff(base: &BTreeMap<String, DepRecord>, head: &BTreeMap<String, DepRecord>) -> DepDiff {
    let mut result = DepDiff::default();

    for (name, old) in base {
        let Some(new) = head.get(name) else {
            let label = kind_label(old.kind, old.optional);
            if label == "runtime" {
                result.breaking += 1;
            }
            result.removed.push(DepRemoved {
                name: name.clone(),
                version: old.ver.clone(),
                kind: label,
            });
            continue;
        };

        if old == new {
            continue;
        }

        let bump = classify_bump(&old.ver, &new.ver);
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

        if label == "runtime"
            && (bump == Bump::Major || features.split(',').any(|token| token.starts_with('-')))
        {
            result.breaking += 1;
        }

        result.changed.push(DepChanged {
            name: name.clone(),
            old: old.ver.clone(),
            new: new.ver.clone(),
            bump,
            kind: label,
            features,
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
