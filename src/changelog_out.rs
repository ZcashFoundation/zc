//! Keep a Changelog document rendering.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::changelog;
use crate::ctx::Ctx;
use crate::model::{CrateChange, CrateResult, DepKind, PerCrateDep, Report, Scope, Section};
use crate::traitmap::{self, TraitMap};

#[derive(Default, Debug, PartialEq, Eq)]
struct DependencyMarkdown {
    added: BTreeMap<String, Vec<String>>,
    changed: BTreeMap<String, Vec<String>>,
    removed: BTreeMap<String, Vec<String>>,
}

fn dependency_key(row: &PerCrateDep) -> (&str, &str, Option<&str>, &str) {
    (
        &row.crate_name,
        &row.dep,
        row.target.as_deref(),
        row.kind.as_str(),
    )
}

fn dependency_context(target: Option<&str>, kind: DepKind) -> String {
    let mut context = String::new();
    if let Some(target) = target {
        context.push_str(&format!(" on `{target}`"));
    }
    if kind == DepKind::Build {
        context.push_str(" (build dependency)");
    }
    context
}

fn dependency_markdown(base: &[PerCrateDep], head: &[PerCrateDep]) -> DependencyMarkdown {
    let base_by_key: HashMap<_, _> = base.iter().map(|row| (dependency_key(row), row)).collect();
    let head_keys: HashSet<_> = head.iter().map(dependency_key).collect();
    let mut markdown = DependencyMarkdown::default();

    for row in head.iter().filter(|row| row.kind != DepKind::Dev) {
        let context = dependency_context(row.target.as_deref(), row.kind);
        let Some(old) = base_by_key.get(&dependency_key(row)) else {
            if row.scope != Scope::Msrv {
                markdown
                    .added
                    .entry(row.crate_name.clone())
                    .or_default()
                    .push(format!("- `{}` dependency{context}.", row.dep));
            }
            continue;
        };
        if old.req == row.req {
            continue;
        }
        let line = match row.scope {
            Scope::Msrv if row.req != "-" => Some(format!("- MSRV is now {}.", row.req)),
            Scope::Msrv => None,
            Scope::Internal => Some(format!(
                "- `{}` dependency bumped to `{}`{context}.",
                row.dep, row.req
            )),
            Scope::External => Some(format!("- Migrated to `{} {}`{context}.", row.dep, row.req)),
        };
        if let Some(line) = line {
            let lines = markdown.changed.entry(row.crate_name.clone()).or_default();
            if row.scope == Scope::Msrv {
                lines.insert(0, line);
            } else {
                lines.push(line);
            }
        }
    }

    for row in base.iter().filter(|row| row.kind != DepKind::Dev) {
        if row.dep != "~msrv" && !head_keys.contains(&dependency_key(row)) {
            markdown
                .removed
                .entry(row.crate_name.clone())
                .or_default()
                .push(format!(
                    "- `{}` dependency{}.",
                    row.dep,
                    dependency_context(row.target.as_deref(), row.kind)
                ));
        }
    }
    markdown
}

fn fold_public_dependency_notes(
    report: &Report,
    dependency: &mut DependencyMarkdown,
) -> BTreeMap<String, Vec<String>> {
    let mut extra = BTreeMap::<String, Vec<String>>::new();
    for result in &report.crates {
        for finding in &result.pubdep {
            let note = if finding.old_package != finding.new_package {
                format!(
                    "package `{}` was replaced by `{}`, changing the identity of types \
                     in this crate's public API; downstream users must migrate in lockstep.",
                    finding.old_package, finding.new_package
                )
            } else if finding.is_breaking() {
                format!(
                    "its types appear in this crate's public API, so downstream users must \
                     upgrade `{}` in lockstep.",
                    finding.dep
                )
            } else {
                "its types appear in this crate's public API, so check whether downstream users \
                 are affected."
                    .to_string()
            };
            let context = dependency_context(finding.target.as_deref(), DepKind::Runtime);
            let migrated = format!("- Migrated to `{} {}`{context}.", finding.dep, finding.new);
            let replacement = format!(
                "- Migrated to `{} {}`{context}; {note}",
                finding.dep, finding.new
            );
            let mut folded = false;
            if let Some(lines) = dependency.changed.get_mut(&result.name) {
                if let Some(line) = lines.iter_mut().find(|line| line.contains(&migrated)) {
                    *line = line.replacen(&migrated, &replacement, 1);
                    folded = true;
                }
            }
            if !folded {
                extra.entry(result.name.clone()).or_default().push(format!(
                    "- Public dependency `{}` changed to `{}`{context}; {note}",
                    finding.dep, finding.new
                ));
            }
        }
    }
    extra
}

fn append_section(out: &mut String, heading: &str, parts: &[&[String]]) {
    if parts.iter().all(|lines| lines.is_empty()) {
        return;
    }
    out.push_str("### ");
    out.push_str(heading);
    out.push('\n');
    for lines in parts {
        for line in *lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push('\n');
}

fn semantic_changes(result: &CrateResult, prefix: &str) -> Vec<String> {
    if result.breaking_added.is_empty() {
        return Vec::new();
    }
    // ponytail: notes may overlap raw additions; consolidate them when curating the draft.
    let prefix = format!("{prefix}::");
    result
        .breaking_added
        .iter()
        .map(|reason| {
            if let Some((path, detail)) = reason.split_once(": ") {
                format!(
                    "- Public API `{}`: {detail}.",
                    path.strip_prefix(&prefix).unwrap_or(path)
                )
            } else {
                format!("- {reason}")
            }
        })
        .collect()
}

pub fn emit(ctx: &Ctx, report: &Report, base: &[PerCrateDep], head: &[PerCrateDep]) -> String {
    let mut dependency = dependency_markdown(base, head);
    let public_dependency = fold_public_dependency_notes(report, &mut dependency);

    let changed_crates: Vec<String> = report
        .crates
        .iter()
        .filter(|result| {
            result.change == CrateChange::Existing
                && result.removed + result.changed + result.added > 0
        })
        .map(|result| result.name.clone())
        .collect();
    let removed_crates: Vec<String> = report
        .crates
        .iter()
        .filter(|result| result.change == CrateChange::Existing && result.removed > 0)
        .map(|result| result.name.clone())
        .collect();

    let (head_traits, base_traits) = if changed_crates.is_empty() {
        (HashMap::new(), HashMap::new())
    } else {
        ctx.progress.start();
        let head_traits = traitmap::dump(ctx, &ctx.refs.head_sha, &changed_crates);
        let base_traits = if removed_crates.is_empty() {
            HashMap::new()
        } else {
            traitmap::dump(ctx, &ctx.refs.baseline_sha, &removed_crates)
        };
        ctx.progress.clear();
        (head_traits, base_traits)
    };

    let empty_traits = TraitMap::new();
    let mut out = String::new();
    for result in &report.crates {
        if result.change != CrateChange::Existing {
            out.push_str(&format!("## {}\n\n", result.name));
            if result.change == CrateChange::Added {
                out.push_str("### Added\n- Initial release!\n\n");
            } else {
                out.push_str("### Removed\n- Crate removed from the workspace.\n\n");
            }
            continue;
        }
        let prefix = result.prefix();
        let head_map = head_traits.get(&result.name).unwrap_or(&empty_traits);
        let base_map = base_traits.get(&result.name).unwrap_or(&empty_traits);
        let added = if result.added > 0 {
            changelog::render(&result.added_lines, Section::Added, &prefix, head_map)
        } else {
            Vec::new()
        };
        let changed_api = if result.changed > 0 {
            changelog::render(&result.changed_lines, Section::Changed, &prefix, head_map)
        } else {
            Vec::new()
        };
        let semantic_api = semantic_changes(result, &prefix);
        let removed_api = if result.removed > 0 {
            changelog::render(&result.removed_lines, Section::Removed, &prefix, base_map)
        } else {
            Vec::new()
        };
        let dep_added = dependency
            .added
            .get(&result.name)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let dep_changed = dependency
            .changed
            .get(&result.name)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let pubdep = public_dependency
            .get(&result.name)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let dep_removed = dependency
            .removed
            .get(&result.name)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let has_changed = !dep_changed.is_empty()
            || !pubdep.is_empty()
            || !changed_api.is_empty()
            || !semantic_api.is_empty();
        let has_removed = !removed_api.is_empty() || !dep_removed.is_empty();
        if added.is_empty() && dep_added.is_empty() && !has_changed && !has_removed {
            continue;
        }

        out.push_str("## ");
        out.push_str(&result.name);
        out.push_str("\n\n");
        append_section(&mut out, "Added", &[&added, dep_added]);
        if has_changed {
            append_section(
                &mut out,
                "Changed",
                &[dep_changed, pubdep, &changed_api, &semantic_api],
            );
        }
        if has_removed {
            append_section(&mut out, "Removed", &[&removed_api, dep_removed]);
        }
    }
    out
}

#[cfg(test)]
#[path = "changelog_out/tests.rs"]
mod tests;
