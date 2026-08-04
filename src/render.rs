//! Human-readable report rendering.

use std::collections::HashMap;

use crate::ctx::Ctx;
use crate::group;
use crate::model::{
    Bump, CrateResult, CrateStatus, DepDiff, GroupMode, GroupRecord, LockDiff, Report, Section,
};
use crate::style::Style;
use crate::version_req;

pub fn header(ctx: &Ctx) {
    println!(
        "{}Comparing public API: {}{}{} ({}){} {}-> {}{}{} ({}){}",
        ctx.style.bold,
        ctx.refs.baseline_label,
        ctx.style.reset,
        ctx.style.dim,
        ctx.refs.baseline_short,
        ctx.style.reset,
        ctx.style.bold,
        ctx.refs.head_label,
        ctx.style.reset,
        ctx.style.dim,
        ctx.refs.head_short,
        ctx.style.reset
    );
    println!();
}

pub fn dep_section(ctx: &Ctx, deps: &DepDiff) {
    if deps.removed.is_empty() && deps.changed.is_empty() && deps.added.is_empty() {
        return;
    }
    let s = &ctx.style;
    println!(
        "{}Dependency changes{}{} (kind: runtime = consumer-visible, build/dev = internal){}",
        s.bold, s.reset, s.dim, s.reset
    );
    println!();

    if !deps.removed.is_empty() {
        println!("  {}Removed ({}):{}", s.red, deps.removed.len(), s.reset);
        for dep in &deps.removed {
            let color = match dep.kind.as_str() {
                "runtime" => s.red,
                "runtime-opt" | "build" => s.yellow,
                _ => s.dim,
            };
            println!(
                "    {}- {} {}{} {}[{}]{}",
                color, dep.name, dep.version, s.reset, s.dim, dep.kind, s.reset
            );
        }
        println!();
    }

    if !deps.changed.is_empty() {
        println!("  {}Changed ({}):{}", s.yellow, deps.changed.len(), s.reset);
        let width = deps
            .changed
            .iter()
            .map(|dep| dep.name.chars().count())
            .max()
            .unwrap_or(0);
        for dep in &deps.changed {
            let color = if dep.bump == Bump::Major && dep.kind == "runtime" {
                s.red
            } else if matches!(dep.kind.as_str(), "dev" | "build" | "runtime-opt") {
                s.dim
            } else {
                s.yellow
            };
            if dep.old == dep.new {
                print!(
                    "    {}{:<width$}  {}{} {}[{}]{}",
                    color,
                    dep.name,
                    dep.old,
                    s.reset,
                    s.dim,
                    dep.kind,
                    s.reset,
                    width = width
                );
            } else {
                print!(
                    "    {}{:<width$}  {} -> {}  ({}){} {}[{}]{}",
                    color,
                    dep.name,
                    dep.old,
                    dep.new,
                    dep.bump,
                    s.reset,
                    s.dim,
                    dep.kind,
                    s.reset,
                    width = width
                );
            }
            if !dep.features.is_empty() {
                print!(" {}features:{} {}", s.dim, s.reset, dep.features);
            }
            println!();
        }
        println!();
    }

    if !deps.added.is_empty() {
        println!("  {}Added ({}):{}", s.green, deps.added.len(), s.reset);
        for dep in &deps.added {
            println!(
                "    {}+ {} {}{} {}[{}]{}",
                s.green, dep.name, dep.version, s.reset, s.dim, dep.kind, s.reset
            );
        }
        println!();
    }
}

fn via_suffix(style: &Style, via: Option<&str>) -> String {
    match via.filter(|value| !value.is_empty()) {
        Some(value) => format!(" {}via {}{}", style.dim, value, style.reset),
        None => String::new(),
    }
}

pub fn lock_section(ctx: &Ctx, lock: &LockDiff) {
    if lock.removed.is_empty() && lock.changed.is_empty() && lock.added.is_empty() {
        return;
    }
    let s = &ctx.style;
    println!(
        "{}Transitive (Cargo.lock) changes{}{} (direct deps already reported above){}",
        s.bold, s.reset, s.dim, s.reset
    );
    println!();

    if !lock.changed.is_empty() {
        println!("  {}Changed ({}):{}", s.yellow, lock.changed.len(), s.reset);
        let width = lock
            .changed
            .iter()
            .map(|(name, _, _)| name.chars().count())
            .max()
            .unwrap_or(0);
        for (name, old, new) in &lock.changed {
            println!(
                "    {}{:<width$}  {} -> {}{}{}",
                s.yellow,
                name,
                old,
                new,
                s.reset,
                via_suffix(s, lock.via.get(name).map(String::as_str)),
                width = width
            );
        }
        println!();
    }
    if !lock.added.is_empty() {
        println!("  {}Added ({}):{}", s.green, lock.added.len(), s.reset);
        for (name, version) in &lock.added {
            println!(
                "    {}+ {} {}{}{}",
                s.green,
                name,
                version,
                s.reset,
                via_suffix(s, lock.via.get(name).map(String::as_str))
            );
        }
        println!();
    }
    if !lock.removed.is_empty() {
        println!("  {}Removed ({}):{}", s.red, lock.removed.len(), s.reset);
        for (name, version) in &lock.removed {
            println!("    {}- {} {}{}", s.red, name, version, s.reset);
        }
        println!();
    }
}

pub fn api_rows(ctx: &Ctx, crates: &[CrateResult]) {
    let s = &ctx.style;
    let width = crates
        .iter()
        .map(|result| result.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(16);
    let mut rows = String::new();
    for result in crates {
        if result.status == CrateStatus::Error {
            let error = match &result.error {
                Some(error) => error,
                None => continue,
            };
            let first = error
                .stderr
                .lines()
                .next()
                .filter(|line| !line.is_empty())
                .unwrap_or("cargo public-api failed");
            rows.push_str(&format!(
                "  {:<width$} {}error{}: {}{} ({}){}\n",
                result.name,
                s.red,
                s.reset,
                error.stage.as_str(),
                s.dim,
                first,
                s.reset,
                width = width
            ));
        } else if result.total() > 0 {
            let tag = if result.removed == 0 && result.changed == 0 {
                format!(" {}(additive){}", s.dim, s.reset)
            } else {
                String::new()
            };
            rows.push_str(&format!(
                "  {:<width$} {}-{}{}  {}~{}{}  {}+{}{}{}\n",
                result.name,
                s.red,
                result.removed,
                s.reset,
                s.yellow,
                result.changed,
                s.reset,
                s.green,
                result.added,
                s.reset,
                tag,
                width = width
            ));
        }
    }
    if !rows.is_empty() {
        println!("{}Public API changes{}", s.bold, s.reset);
        println!();
        print!("{rows}");
    }
}

pub fn summary(ctx: &Ctx, report: &Report) {
    println!();
    println!(
        "{}Summary{}  {}({}/{} crates with changes){}",
        ctx.style.bold,
        ctx.style.reset,
        ctx.style.dim,
        report.changed_crate_count,
        report.crate_count,
        ctx.style.reset
    );
}

fn render_items(
    style: &Style,
    lines: &[String],
    section: Section,
    mode: GroupMode,
    crate_prefix: &str,
    src_kinds: &HashMap<String, String>,
) {
    if mode == GroupMode::Flat {
        for line in lines.iter().filter(|line| !line.is_empty()) {
            match section {
                Section::Removed => println!("    {}- {}{}", style.red, line, style.reset),
                Section::Added => println!("    {}+ {}{}", style.green, line, style.reset),
                Section::Changed => {
                    if let Some(item) = line.strip_prefix("  - ") {
                        println!("    {}- {}{}", style.red, item, style.reset);
                    } else if let Some(item) = line.strip_prefix("  + ") {
                        println!("    {}+ {}{}", style.green, item, style.reset);
                    } else {
                        println!("    {line}");
                    }
                }
            }
        }
        return;
    }

    let records = group::group(lines, section, mode, crate_prefix, src_kinds);
    let mut previous: Option<&GroupRecord> = None;
    for record in &records {
        match record {
            GroupRecord::TypeHeader { name, kind } => {
                if previous.is_some() {
                    println!();
                }
                let name = if name.is_empty() { "(other)" } else { name };
                if kind.is_empty() {
                    println!("    {}{}{}", style.dim, name, style.reset);
                } else {
                    println!("    {}{}  ({}){}", style.dim, name, kind, style.reset);
                }
            }
            GroupRecord::ModHeader(name) => {
                if previous.is_some() {
                    println!();
                }
                let name = if name.is_empty() { "(other)" } else { name };
                println!("    {}{}{}", style.dim, name, style.reset);
            }
            GroupRecord::ExtDivider => {
                println!();
                println!(
                    "    {}[trait impls on external types]{}",
                    style.dim, style.reset
                );
            }
            GroupRecord::TypeSub { name, kind } => {
                if previous.is_some() && !matches!(previous, Some(GroupRecord::ModHeader(_))) {
                    println!();
                }
                println!("      {}{}  ({}){}", style.dim, name, kind, style.reset);
            }
            GroupRecord::Item(item) | GroupRecord::DeepItem(item) => {
                let deep = matches!(record, GroupRecord::DeepItem(_));
                if !deep
                    && matches!(
                        previous,
                        Some(GroupRecord::TypeSub { .. } | GroupRecord::DeepItem(_))
                    )
                {
                    println!();
                }
                let indent = if deep { "        " } else { "      " };
                match section {
                    Section::Removed => {
                        println!("{}{}- {}{}", indent, style.red, item, style.reset)
                    }
                    Section::Added => {
                        println!("{}{}+ {}{}", indent, style.green, item, style.reset)
                    }
                    Section::Changed => {
                        if let Some(value) = item.strip_prefix("  - ") {
                            println!("{}{}- {}{}", indent, style.red, value, style.reset);
                        } else if let Some(value) = item.strip_prefix("  + ") {
                            println!("{}{}+ {}{}", indent, style.green, value, style.reset);
                        } else {
                            println!("{indent}{item}");
                        }
                    }
                }
            }
        }
        previous = Some(record);
    }
}

pub fn details(ctx: &Ctx, report: &Report, src_kinds: &HashMap<String, String>) {
    for result in report.crates.iter().filter(|result| result.total() > 0) {
        println!();
        println!("{}{}{}", ctx.style.bold, result.name, ctx.style.reset);
        let prefix = result.prefix();
        if result.removed > 0 {
            println!();
            println!(
                "  {}Removed ({}):{}",
                ctx.style.red, result.removed, ctx.style.reset
            );
            render_items(
                &ctx.style,
                &result.removed_lines,
                Section::Removed,
                ctx.opts.group_mode,
                &prefix,
                src_kinds,
            );
        }
        if result.changed > 0 {
            println!();
            println!(
                "  {}Changed ({}):{}",
                ctx.style.yellow, result.changed, ctx.style.reset
            );
            render_items(
                &ctx.style,
                &result.changed_lines,
                Section::Changed,
                ctx.opts.group_mode,
                &prefix,
                src_kinds,
            );
        }
        if result.added > 0 {
            println!();
            println!(
                "  {}Added ({}):{}",
                ctx.style.green, result.added, ctx.style.reset
            );
            render_items(
                &ctx.style,
                &result.added_lines,
                Section::Added,
                ctx.opts.group_mode,
                &prefix,
                src_kinds,
            );
        }
    }
}

pub fn values_section(ctx: &Ctx, report: &Report) {
    let s = &ctx.style;
    if !report.values.is_empty() {
        println!();
        println!(
            "{}Value changes ({}){}{} — const/static values; cargo-public-api can't see these{}",
            s.bold,
            report.values.len(),
            s.reset,
            s.dim,
            s.reset
        );
        let mut last_crate = "";
        for change in &report.values {
            if change.crate_name != last_crate {
                println!();
                println!("  {}{}{}", s.bold, change.crate_name, s.reset);
                last_crate = &change.crate_name;
            }
            println!(
                "    {}~ {}: {}{}",
                s.yellow, change.path, change.ty, s.reset
            );
            println!(
                "        {}{}{} {}->{} {}{}{}",
                s.red, change.old, s.reset, s.dim, s.reset, s.green, change.new, s.reset
            );
        }
    }
    if !report.docs.is_empty() {
        println!();
        println!(
            "{}Doc changes ({}){}{} — public doc-comment text changed{}",
            s.bold,
            report.docs.len(),
            s.reset,
            s.dim,
            s.reset
        );
        let mut last_crate = "";
        for change in &report.docs {
            if change.crate_name != last_crate {
                println!();
                println!("  {}{}{}", s.bold, change.crate_name, s.reset);
                last_crate = &change.crate_name;
            }
            println!(
                "    {}~ {}{}{} (doc text changed){}",
                s.yellow, change.path, s.reset, s.dim, s.reset
            );
        }
    }
}

pub fn pubdep_section(ctx: &Ctx, report: &Report) {
    if report.pubdep_break_total + report.pubdep_review_total == 0 {
        return;
    }
    let s = &ctx.style;
    println!();
    println!(
        "{}Public-dependency changes ({} breaking, {} to review){}{} — public API exposes a changed dependency; cargo-public-api can't see these{}",
        s.bold,
        report.pubdep_break_total,
        report.pubdep_review_total,
        s.reset,
        s.dim,
        s.reset
    );
    for result in report
        .crates
        .iter()
        .filter(|result| !result.pubdep.is_empty())
    {
        println!();
        println!("  {}{}{}", s.bold, result.name, s.reset);
        for finding in &result.pubdep {
            if version_req::classify_bump(&finding.old, &finding.new) == Bump::Major {
                println!(
                    "    {}{}{}{}: {}{}{}{} {}->{} {}{}{}{} (incompatible; reachable in public API){}",
                    s.red,
                    finding.dep,
                    s.reset,
                    s.dim,
                    s.reset,
                    s.red,
                    finding.old,
                    s.reset,
                    s.dim,
                    s.reset,
                    s.green,
                    finding.new,
                    s.reset,
                    s.dim,
                    s.reset
                );
            } else {
                println!(
                    "    {}{}{}{}: {}{}{}{} {}->{} {}{}{}{} (compatibility unclear; reachable in public API){}",
                    s.yellow,
                    finding.dep,
                    s.reset,
                    s.dim,
                    s.reset,
                    s.yellow,
                    finding.old,
                    s.reset,
                    s.dim,
                    s.reset,
                    s.green,
                    finding.new,
                    s.reset,
                    s.dim,
                    s.reset
                );
            }
        }
    }
}

pub fn verdict(ctx: &Ctx, report: &Report) {
    let s = &ctx.style;
    if report.any_breaking() {
        let mut parts = Vec::new();
        if report.api_breaking() > 0 {
            parts.push(format!(
                "api: {} removed / {} changed",
                report.removed_total, report.changed_total
            ));
        }
        if report.deps.breaking > 0 {
            parts.push(format!("runtime-deps: {} breaking", report.deps.breaking));
        }
        if !report.values.is_empty() {
            parts.push(format!("values: {} changed", report.values.len()));
        }
        if report.pubdep_break_total > 0 {
            parts.push(format!(
                "public-dep: {} breaking",
                report.pubdep_break_total
            ));
        }
        println!(
            "{}{}BREAKING{}{}: {}.{}",
            s.red,
            s.bold,
            s.reset,
            s.red,
            parts.join("; "),
            s.reset
        );
        if report.added_total > 0 {
            println!(
                "{}(also {} new API items, additive only){}",
                s.dim, report.added_total, s.reset
            );
        }
        if !report.docs.is_empty() {
            println!(
                "{}(also {} public doc-comment change(s)){}",
                s.dim,
                report.docs.len(),
                s.reset
            );
        }
    } else {
        if report.added_total > 0 {
            println!(
                "{}OK: {} new API items, no breaking changes.{}",
                s.green, report.added_total, s.reset
            );
        } else {
            println!("{}OK: no breaking changes.{}", s.green, s.reset);
        }
        if !report.docs.is_empty() {
            println!(
                "{}(also {} public doc-comment change(s)){}",
                s.dim,
                report.docs.len(),
                s.reset
            );
        }
    }
}

pub fn api_errors(ctx: &Ctx, report: &Report) {
    let s = &ctx.style;
    eprintln!(
        "{}{}ERROR{}{}: cargo-public-api failed for {} crate(s).{}",
        s.red, s.bold, s.reset, s.red, report.error_crate_count, s.reset
    );
    for result in report
        .crates
        .iter()
        .filter(|result| result.status == CrateStatus::Error)
    {
        let error = match &result.error {
            Some(error) => error,
            None => continue,
        };
        eprintln!();
        eprintln!("  {}{}{}", s.bold, result.name, s.reset);
        eprintln!("    stage: {}", error.stage.as_str());
        eprintln!("    ref: {} ({})", error.ref_label, error.ref_sha);
        eprintln!("    command: {}", error.command);
        eprintln!("    hint: {}", error.hint);
        eprintln!("    stderr:");
        if error.stderr.is_empty() {
            eprintln!("      ");
        } else {
            for line in error.stderr.lines() {
                eprintln!("      {line}");
            }
        }
    }
}

#[cfg(test)]
#[path = "render/tests.rs"]
mod tests;
