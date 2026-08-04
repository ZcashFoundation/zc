//! Librustzcash-style changelog document rendering.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::changelog;
use crate::ctx::Ctx;
use crate::model::{Bump, PerCrateDep, Report, Scope, Section};
use crate::traitmap::{self, TraitMap};
use crate::version_req;

#[derive(Default, Debug, PartialEq, Eq)]
struct DependencyMarkdown {
    changed: BTreeMap<String, Vec<String>>,
    removed: BTreeMap<String, Vec<String>>,
}

fn dependency_markdown(base: &[PerCrateDep], head: &[PerCrateDep]) -> DependencyMarkdown {
    let base_by_key: HashMap<(&str, &str), &PerCrateDep> = base
        .iter()
        .map(|row| ((row.crate_name.as_str(), row.dep.as_str()), row))
        .collect();
    let head_keys: HashSet<(&str, &str)> = head
        .iter()
        .map(|row| (row.crate_name.as_str(), row.dep.as_str()))
        .collect();
    let mut markdown = DependencyMarkdown::default();

    for row in head {
        let Some(old) = base_by_key.get(&(row.crate_name.as_str(), row.dep.as_str())) else {
            continue;
        };
        if old.req == row.req {
            continue;
        }
        let line = match row.scope {
            Scope::Msrv if row.req != "-" => Some(format!("- MSRV is now {}.", row.req)),
            Scope::Msrv => None,
            Scope::Internal => Some(format!(
                "- `{}` dependency bumped to `{}`.",
                row.dep, row.req
            )),
            Scope::External => Some(format!("- Migrated to `{} {}`.", row.dep, row.req)),
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

    for row in base {
        if row.dep != "~msrv" && !head_keys.contains(&(row.crate_name.as_str(), row.dep.as_str())) {
            markdown
                .removed
                .entry(row.crate_name.clone())
                .or_default()
                .push(format!("- `{}` dependency.", row.dep));
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
            let note = if version_req::classify_bump(&finding.old, &finding.new) == Bump::Major {
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
            let migrated = format!("- Migrated to `{} {}`.", finding.dep, finding.new);
            let replacement = format!("- Migrated to `{} {}`; {note}", finding.dep, finding.new);
            let mut folded = false;
            if let Some(lines) = dependency.changed.get_mut(&result.name) {
                if let Some(line) = lines.iter_mut().find(|line| line.contains(&migrated)) {
                    *line = line.replacen(&migrated, &replacement, 1);
                    folded = true;
                }
            }
            if !folded {
                extra.entry(result.name.clone()).or_default().push(format!(
                    "- Public dependency `{}` changed to `{}`; {note}",
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

pub fn emit(ctx: &Ctx, report: &Report, base: &[PerCrateDep], head: &[PerCrateDep]) -> String {
    let mut dependency = dependency_markdown(base, head);
    let public_dependency = fold_public_dependency_notes(report, &mut dependency);

    let changed_crates: Vec<String> = report
        .crates
        .iter()
        .filter(|result| result.total() > 0)
        .map(|result| result.name.clone())
        .collect();
    let removed_crates: Vec<String> = report
        .crates
        .iter()
        .filter(|result| result.removed > 0)
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
        let removed_api = if result.removed > 0 {
            changelog::render(&result.removed_lines, Section::Removed, &prefix, base_map)
        } else {
            Vec::new()
        };
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
        let has_changed = !dep_changed.is_empty() || !pubdep.is_empty() || !changed_api.is_empty();
        let has_removed = !removed_api.is_empty() || !dep_removed.is_empty();
        if added.is_empty() && !has_changed && !has_removed {
            continue;
        }

        out.push_str("## ");
        out.push_str(&result.name);
        out.push_str("\n\n");
        append_section(&mut out, "Added", &[&added]);
        if has_changed {
            append_section(&mut out, "Changed", &[dep_changed, pubdep, &changed_api]);
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
