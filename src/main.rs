//! zc — detect public API and dependency changes across all workspace crates between two
//! git refs.
//!
//! Git, Cargo, rustdoc and cargo-public-api stay subprocesses: they are the source of truth
//! for refs, feature resolution and API surfaces. Everything above them — parsing,
//! classification, grouping and rendering — is Rust.

mod api;
mod cache;
mod cargo_meta;
mod changelog;
mod changelog_out;
mod ctx;
mod deps;
mod git;
mod github;
mod group;
mod json;
mod lock;
mod model;
mod progress;
mod pubdep;
mod render;
mod report_file;
mod style;
mod traitmap;
mod values;
mod version_req;

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;

use ctx::Ctx;
use model::{FailOn, GroupMode, Options, Refs, Report, EXIT_ANALYSIS, EXIT_OK, EXIT_USAGE};
use progress::Progress;
use style::Style;

const HELP: &str = include_str!("help.txt");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(EXIT_ANALYSIS as u8)
        }
    }
}

/// A parsed command line.
struct Args {
    opts: Options,
    positional: Vec<String>,
}

fn parse_args(argv: Vec<String>, style: &Style) -> Result<Args, (String, i32)> {
    let mut opts = Options {
        with_lock: false,
        with_values: false,
        json_mode: false,
        changelog_mode: false,
        group_mode: GroupMode::Mod,
        fail_on: FailOn::Breaking,
        report_path: None,
    };
    let mut positional = Vec::new();
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err((HELP.to_string(), EXIT_OK)),
            "-V" | "--version" => return Err((format!("zc {VERSION}"), EXIT_OK)),
            "--with-lock" => opts.with_lock = true,
            "--flat" => opts.group_mode = GroupMode::Flat,
            "--by-type" => opts.group_mode = GroupMode::Type,
            "--with-values" => opts.with_values = true,
            "--json" => opts.json_mode = true,
            "--changelog" => opts.changelog_mode = true,
            "--fail-on" => {
                let Some(value) = it.next() else {
                    return Err((missing_value(style, "--fail-on"), EXIT_USAGE));
                };
                let Some(fail_on) = FailOn::parse(&value) else {
                    return Err((
                        format!(
                            "{}error:{} invalid --fail-on value '{value}', expected breaking, \
                             api-breaking, error, or none (run with --help for usage)",
                            style.red, style.reset
                        ),
                        EXIT_USAGE,
                    ));
                };
                opts.fail_on = fail_on;
            }
            "--report" => {
                let Some(path) = it.next() else {
                    return Err((missing_value(style, "--report"), EXIT_USAGE));
                };
                opts.report_path = Some(PathBuf::from(path));
            }
            "--" => {
                positional.extend(it);
                break;
            }
            other if other.starts_with('-') => {
                return Err((
                    format!(
                        "{}error:{} unknown option '{other}' (run with --help for usage)",
                        style.red, style.reset
                    ),
                    EXIT_USAGE,
                ))
            }
            other => {
                positional.push(other.to_string());
                positional.extend(it);
                break;
            }
        }
    }
    if positional.len() > 2 {
        return Err((
            format!(
                "{}error:{} too many positional arguments, expected at most 2 \
                 (run with --help for usage)",
                style.red, style.reset
            ),
            EXIT_USAGE,
        ));
    }
    Ok(Args { opts, positional })
}

fn missing_value(style: &Style, option: &str) -> String {
    format!(
        "{}error:{} '{option}' needs a value (run with --help for usage)",
        style.red, style.reset
    )
}

/// Emit the GitHub Actions view of the finished analysis and apply the exit-code policy.
fn finish(ctx: &Ctx, report: &Report) -> i32 {
    if github::is_active() {
        github::emit(report, ctx.opts.fail_on);
    }
    ctx.opts.fail_on.exit_code(report)
}

fn run() -> Result<i32, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // Style for pre-parse diagnostics: assume a document mode only once we know.
    let probe = Style::detect(false);
    let args = match parse_args(argv, &probe) {
        Ok(args) => args,
        Err((msg, EXIT_OK)) => {
            println!("{msg}");
            return Ok(EXIT_OK);
        }
        Err((msg, code)) => {
            eprintln!("{msg}");
            return Ok(code);
        }
    };
    let opts = args.opts;
    let document_mode = opts.json_mode || opts.changelog_mode;
    let style = Style::detect(document_mode);

    if let Some(path) = &opts.report_path {
        if let Err(message) = report_file::check_dir(path) {
            eprintln!(
                "{}error:{} {message} (run with --help for usage)",
                style.red, style.reset
            );
            return Ok(EXIT_USAGE);
        }
    }

    // ── ref resolution ────────────────────────────────────────────────
    let positional = args.positional;
    let dirty = positional.len() < 2 && git::is_worktree_dirty();

    let (mut baseline, mut head_ref, mut head_label, use_merge_base) = match positional.len() {
        2 => (
            positional[0].clone(),
            positional[1].clone(),
            positional[1].clone(),
            false,
        ),
        1 if dirty => (
            positional[0].clone(),
            String::new(),
            "working tree".to_string(),
            true,
        ),
        1 => (
            positional[0].clone(),
            "HEAD".to_string(),
            "HEAD".to_string(),
            true,
        ),
        _ if dirty => (
            "HEAD".to_string(),
            String::new(),
            "working tree".to_string(),
            false,
        ),
        _ => (
            git::detect_parent_branch(),
            "HEAD".to_string(),
            "HEAD".to_string(),
            true,
        ),
    };

    let mut baseline_label = baseline.clone();
    if use_merge_base {
        let head_for_mb = if head_ref.is_empty() {
            "HEAD"
        } else {
            &head_ref
        };
        if let Some(mb) = git::merge_base(&baseline, head_for_mb) {
            baseline_label = format!("merge-base({baseline_label}, {head_label})");
            baseline = mb;
        }
    }

    if !git::rev_parse_ok(&baseline) {
        eprintln!(
            "{}error:{} unknown git ref '{baseline}' (run with --help for usage)",
            style.red, style.reset
        );
        return Ok(EXIT_USAGE);
    }
    if !head_ref.is_empty() && !git::rev_parse_ok(&head_ref) {
        eprintln!(
            "{}error:{} unknown git ref '{head_ref}' (run with --help for usage)",
            style.red, style.reset
        );
        return Ok(EXIT_USAGE);
    }

    // ── prerequisites ─────────────────────────────────────────────────
    let cargo_public_api_version = match api::cargo_public_api_version() {
        Some(v) => v,
        None => {
            eprintln!(
                "{}error:{} cargo-public-api is not installed",
                style.red, style.reset
            );
            eprintln!("  install it with: cargo install cargo-public-api");
            return Ok(EXIT_USAGE);
        }
    };
    let toolchain = match api::nightly_toolchain() {
        Some(t) => t,
        None => {
            eprintln!(
                "{}error:{} zc needs a nightly toolchain to build rustdoc JSON",
                style.red, style.reset
            );
            eprintln!("  install one with: rustup toolchain install nightly");
            return Ok(EXIT_USAGE);
        }
    };
    let rustc_version = match api::rustc_version(&toolchain) {
        Some(v) => v,
        None => {
            eprintln!(
                "{}error:{} selected nightly toolchain '{toolchain}' cannot run rustc",
                style.red, style.reset
            );
            return Ok(EXIT_USAGE);
        }
    };

    // cargo-public-api and the rustdoc JSON builds always run with all features, so
    // feature-gated public items are never silently missing from the diff, trait map, or
    // value/doc index. Kept identical across all of them so they stay in sync.
    let feature_args = vec!["--all-features".to_string()];

    let cache = cache::Cache::new(
        VERSION,
        &cargo_public_api_version,
        &rustc_version,
        &feature_args,
    )
    .map_err(|e| format!("{}error:{} {e}", style.red, style.reset))?;
    let tmp =
        cache::RunTmp::new().map_err(|e| format!("{}error:{} {e}", style.red, style.reset))?;
    git::worktree_prune();
    cache.prune_old_api_json();

    // A dirty working tree is snapshotted into an unreachable commit, so the API diff can
    // address it as a ref without touching the tree, index, or stash.
    let mut head_is_worktree_snapshot = false;
    if head_ref.is_empty() {
        head_ref = git::worktree_snapshot_commit(&tmp.dir)
            .map_err(|e| format!("{}error:{} {e}", style.red, style.reset))?;
        head_is_worktree_snapshot = true;
    }

    let baseline_short = git::rev_parse_short(&baseline);
    let head_short = git::rev_parse_short(&head_ref);
    let baseline_sha = git::rev_parse_verify(&baseline).map_err(|_| {
        format!(
            "{}error:{} cannot resolve baseline ref '{baseline}'",
            style.red, style.reset
        )
    })?;
    let head_sha = git::rev_parse_verify(&head_ref)
        .map_err(|_| format!("{}error:{} cannot resolve head ref", style.red, style.reset))?;

    let refs = Refs {
        baseline,
        baseline_label,
        baseline_sha,
        baseline_short,
        head_ref,
        head_label: std::mem::take(&mut head_label),
        head_sha,
        head_short,
        head_is_worktree_snapshot,
    };

    let baseline_worktree = tmp.sub("api-baseline")?;
    git::worktree_add(&baseline_worktree, &refs.baseline_sha).map_err(|_| {
        format!(
            "{}error:{} failed to create public-api worktree for '{}'",
            style.red, style.reset, refs.baseline_label
        )
    })?;
    let head_worktree = tmp.sub("api-head")?;
    git::worktree_add(&head_worktree, &refs.head_sha).map_err(|_| {
        format!(
            "{}error:{} failed to create public-api worktree for '{}'",
            style.red, style.reset, refs.head_label
        )
    })?;

    let baseline_target = tmp.dir.join("api-baseline-target");
    let head_target = tmp.dir.join("api-head-target");
    for dir in [&baseline_target, &head_target] {
        std::fs::create_dir_all(dir).map_err(|_| {
            format!(
                "{}error:{} failed to create public-api target dirs",
                style.red, style.reset
            )
        })?;
    }

    let ctx = Ctx {
        opts,
        style,
        cache,
        tmp,
        toolchain,
        feature_args,
        refs,
        progress: Progress::new(),
        baseline_worktree,
        head_worktree,
        baseline_target,
        head_target,
    };

    if !document_mode {
        render::header(&ctx);
    }

    // ── workspace dependency diff ─────────────────────────────────────
    let base_deps = cargo_meta::dump_workspace_deps(&ctx, &ctx.refs.baseline)
        .map_err(|e| format!("{}error:{} {e}", ctx.style.red, ctx.style.reset))?;
    let head_deps = cargo_meta::dump_workspace_deps(&ctx, &ctx.refs.head_ref)
        .map_err(|e| format!("{}error:{} {e}", ctx.style.red, ctx.style.reset))?;
    let dep_diff = deps::diff(&base_deps, &head_deps);
    if !document_mode {
        render::dep_section(&ctx, &dep_diff);
    }

    // ── transitive (Cargo.lock) diff ──────────────────────────────────
    let all_crates = cargo_meta::workspace_crate_names(&ctx.head_worktree);
    if all_crates.is_empty() {
        eprintln!(
            "{}warning:{} no workspace crates discovered via cargo metadata",
            ctx.style.yellow, ctx.style.reset
        );
    }
    let crate_count = all_crates.len();

    if ctx.opts.with_lock {
        // Direct workspace deps (and workspace members) are suppressed: the section above
        // already covers them.
        let mut direct: HashSet<String> = HashSet::new();
        for name in base_deps.keys().chain(head_deps.keys()) {
            direct.insert(name.clone());
            if let Some((_, real)) = name.split_once(" (pkg: ") {
                direct.insert(real.trim_end_matches(')').to_string());
            }
        }
        direct.extend(all_crates.iter().cloned());
        if let (Some(diff), false) = (lock::diff(&ctx, &direct).as_ref(), document_mode) {
            render::lock_section(&ctx, diff);
        }
    }

    // ── per-crate API diff ────────────────────────────────────────────
    // Per-crate direct deps at both refs feed the public-dependency semver join. Cached, so
    // --changelog's later read is a hit; failure degrades to an empty join.
    let pubdep_tables = pubdep::PubdepTables::build(
        &cargo_meta::dump_per_crate_deps(&ctx, &ctx.refs.baseline_sha).unwrap_or_default(),
        &cargo_meta::dump_per_crate_deps(&ctx, &ctx.refs.head_sha).unwrap_or_default(),
    );

    ctx.progress.start();
    let crates = api::analyze(&ctx, &all_crates, &pubdep_tables);
    ctx.progress.clear();

    let removed_total: usize = crates.iter().map(|c| c.removed).sum();
    let changed_total: usize = crates.iter().map(|c| c.changed).sum();
    let added_total: usize = crates.iter().map(|c| c.added).sum();
    let changed_crate_count = crates.iter().filter(|c| c.total() > 0).count();
    let error_crate_count = crates
        .iter()
        .filter(|c| c.status == model::CrateStatus::Error)
        .count();

    if !document_mode {
        render::api_rows(&ctx, &crates);
    }

    // ── const/static value + doc-comment diff (--with-values) ─────────
    let (values, docs) = if ctx.opts.with_values {
        values::diff(&ctx, crate_count)
    } else {
        (Vec::new(), Vec::new())
    };

    // A reachable dependency change is only a break when its requirement change is provably
    // incompatible; an "unknown" one is a review item and never flips the verdict.
    let mut pubdep_break_total = 0;
    let mut pubdep_review_total = 0;
    for c in &crates {
        for f in &c.pubdep {
            match version_req::classify_bump(&f.old, &f.new) {
                model::Bump::Major => pubdep_break_total += 1,
                _ => pubdep_review_total += 1,
            }
        }
    }

    let report = model::Report {
        refs: ctx.refs.clone(),
        deps: dep_diff,
        crates,
        crate_count,
        values,
        docs,
        removed_total,
        changed_total,
        added_total,
        changed_crate_count,
        error_crate_count,
        pubdep_break_total,
        pubdep_review_total,
    };

    if let Some(path) = &ctx.opts.report_path {
        report_file::write(path, &json::emit(&report))
            .map_err(|e| format!("{}error:{} {e}", ctx.style.red, ctx.style.reset))?;
    }

    // ── summary ───────────────────────────────────────────────────────
    if !document_mode {
        render::summary(&ctx, &report);
    }
    let nothing_changed = report.removed_total
        + report.changed_total
        + report.added_total
        + report.values.len()
        + report.docs.len()
        == 0
        && report.error_crate_count == 0
        && report.pubdep_break_total == 0
        && report.pubdep_review_total == 0;
    // In a document mode this falls through: a crate may still have dependency-only
    // changes to document.
    if nothing_changed && !document_mode {
        println!();
        println!(
            "  {}No public API changes.{}",
            ctx.style.green, ctx.style.reset
        );
        // Dep-only changes still deserve a verdict.
        if report.deps.breaking > 0 {
            println!();
            println!(
                "{}{}BREAKING{}{}: runtime-deps: {} breaking.{}",
                ctx.style.red,
                ctx.style.bold,
                ctx.style.reset,
                ctx.style.red,
                report.deps.breaking,
                ctx.style.reset
            );
        }
        return Ok(finish(&ctx, &report));
    }

    // ── changelog document ────────────────────────────────────────────
    if ctx.opts.changelog_mode {
        if report.error_crate_count > 0 {
            render::api_errors(&ctx, &report);
            return Ok(finish(&ctx, &report));
        }
        let base = cargo_meta::dump_per_crate_deps(&ctx, &ctx.refs.baseline_sha).map_err(|_| {
            format!(
                "{}error:{} could not read per-crate dependencies at baseline ({})",
                ctx.style.red, ctx.style.reset, ctx.refs.baseline_sha
            )
        })?;
        let head = cargo_meta::dump_per_crate_deps(&ctx, &ctx.refs.head_sha).map_err(|_| {
            format!(
                "{}error:{} could not read per-crate dependencies at head ({})",
                ctx.style.red, ctx.style.reset, ctx.refs.head_sha
            )
        })?;
        if base.is_empty() || head.is_empty() {
            return Err(format!(
                "{}error:{} per-crate dependency dump was empty for one side (baseline={}, \
                 head={}); refusing to emit a degenerate diff",
                ctx.style.red, ctx.style.reset, ctx.refs.baseline_sha, ctx.refs.head_sha
            ));
        }
        print!("{}", changelog_out::emit(&ctx, &report, &base, &head));
        return Ok(EXIT_OK);
    }

    // ── JSON document ─────────────────────────────────────────────────
    if ctx.opts.json_mode {
        println!("{}", json::emit(&report));
        return Ok(finish(&ctx, &report));
    }

    // ── detailed diffs ────────────────────────────────────────────────
    let src_kinds = if ctx.opts.group_mode == GroupMode::Flat {
        Default::default()
    } else {
        git::type_kinds(&ctx.refs.head_sha)
    };
    render::details(&ctx, &report, &src_kinds);
    render::values_section(&ctx, &report);
    render::pubdep_section(&ctx, &report);

    println!();
    if report.error_crate_count > 0 {
        render::api_errors(&ctx, &report);
        return Ok(finish(&ctx, &report));
    }
    render::verdict(&ctx, &report);
    Ok(finish(&ctx, &report))
}
