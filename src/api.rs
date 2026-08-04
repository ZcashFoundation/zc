//! Rustdoc JSON builds and per-crate public API analysis.

use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde_json::Value;

use crate::ctx::Ctx;
use crate::model::{ApiError, CrateResult, CrateStatus, ErrorStage};
use crate::pubdep::{self, PubdepTables};

/// Returns the installed cargo-public-api version string.
pub fn cargo_public_api_version() -> Option<String> {
    successful_stdout(Command::new("cargo").args(["public-api", "--version"]))
}

/// Returns the selected nightly toolchain, preferring a non-empty `ZC_TOOLCHAIN`.
pub fn nightly_toolchain() -> Option<String> {
    if let Some(toolchain) = std::env::var_os("ZC_TOOLCHAIN") {
        let toolchain = toolchain.to_string_lossy();
        if !toolchain.is_empty() {
            return Some(toolchain.into_owned());
        }
    }

    let output = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|word| word.starts_with("nightly"))
        .map(str::to_owned)
}

/// Returns the rustc version reported by the selected toolchain.
pub fn rustc_version(toolchain: &str) -> Option<String> {
    successful_stdout(
        Command::new("rustc")
            .arg(format!("+{toolchain}"))
            .arg("--version"),
    )
}

fn successful_stdout(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| trim_command_substitution(&String::from_utf8_lossy(&output.stdout)))
}

fn trim_command_substitution(text: &str) -> String {
    text.trim_end_matches('\n').to_owned()
}

/// Builds or loads rustdoc JSON for one crate at one ref.
pub fn rustdoc_json(
    ctx: &Ctx,
    crate_name: &str,
    worktree: &Path,
    target: &Path,
    ref_sha: &str,
) -> Result<PathBuf, String> {
    let cacheable = ctx.api_json_cacheable(ref_sha);
    let cache_name = format!("{}.{}.{}.api.json", ref_sha, ctx.cache.api_fp, crate_name);
    let cache_path = ctx.cache.path(&cache_name);

    if cacheable && valid_cached_json(&cache_path) {
        touch(&cache_path);
        return Ok(cache_path);
    }

    let mut command = Command::new("cargo");
    command
        .arg(format!("+{}", ctx.toolchain))
        .args(["rustdoc", "-q", "--manifest-path"])
        .arg(worktree.join("Cargo.toml"))
        .args(["-p", crate_name, "--lib"])
        .args(&ctx.feature_args)
        .args(["--", "-Z", "unstable-options", "--output-format", "json"])
        .env("CARGO_TARGET_DIR", target);

    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }

    let json_path = target
        .join("doc")
        .join(format!("{}.json", crate_name.replace('-', "_")));
    if !json_path.is_file() {
        return Err(format!("rustdoc did not produce {}", json_path.display()));
    }

    if cacheable {
        ctx.cache.copy_atomic(&cache_name, &json_path);
    }

    Ok(json_path)
}

fn valid_cached_json(path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    if contents.is_empty() {
        return false;
    }

    serde_json::from_str::<Value>(&contents).is_ok_and(|json| {
        json.get("format_version").is_some()
            && json.get("root").is_some()
            && json.get("index").is_some()
    })
}

fn touch(path: &Path) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let times = FileTimes::new().set_modified(SystemTime::now());
    let _ = file.set_times(times);
}

/// Analyzes every crate in input order.
pub fn analyze(ctx: &Ctx, crates: &[String], tables: &PubdepTables) -> Vec<CrateResult> {
    let mut results = Vec::with_capacity(crates.len());

    for (index, crate_name) in crates.iter().enumerate() {
        ctx.progress.set(&format!(
            "public-api: [{}/{}] {}",
            index + 1,
            crates.len(),
            crate_name
        ));

        let base_json = match rustdoc_json(
            ctx,
            crate_name,
            &ctx.baseline_worktree,
            &ctx.baseline_target,
            &ctx.refs.baseline_sha,
        ) {
            Ok(path) => path,
            Err(stderr) => {
                results.push(failure_result(
                    ctx,
                    crate_name,
                    ErrorStage::BaselineBuild,
                    &ctx.refs.baseline_label,
                    &ctx.refs.baseline_sha,
                    &stderr,
                ));
                continue;
            }
        };

        let head_json = match rustdoc_json(
            ctx,
            crate_name,
            &ctx.head_worktree,
            &ctx.head_target,
            &ctx.refs.head_sha,
        ) {
            Ok(path) => path,
            Err(stderr) => {
                results.push(failure_result(
                    ctx,
                    crate_name,
                    ErrorStage::HeadBuild,
                    &ctx.refs.head_label,
                    &ctx.refs.head_sha,
                    &stderr,
                ));
                continue;
            }
        };

        let mut command = Command::new("cargo");
        command
            .arg("public-api")
            .args(&ctx.feature_args)
            .args(["-p", crate_name, "-ss", "diff"])
            .arg(&base_json)
            .arg(&head_json)
            .current_dir(&ctx.tmp.dir);

        let output = match command.output() {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                results.push(failure_result(
                    ctx,
                    crate_name,
                    ErrorStage::Diff,
                    &format!("{}..{}", ctx.refs.baseline_label, ctx.refs.head_label),
                    &format!("{}..{}", ctx.refs.baseline_sha, ctx.refs.head_sha),
                    &stderr,
                ));
                continue;
            }
            Err(error) => {
                results.push(failure_result(
                    ctx,
                    crate_name,
                    ErrorStage::Diff,
                    &format!("{}..{}", ctx.refs.baseline_label, ctx.refs.head_label),
                    &format!("{}..{}", ctx.refs.baseline_sha, ctx.refs.head_sha),
                    &error.to_string(),
                ));
                continue;
            }
        };

        let parsed = parse_diff(&String::from_utf8_lossy(&output.stdout));
        results.push(CrateResult {
            name: crate_name.clone(),
            removed: parsed.removed.len(),
            changed: parsed.changed_old_count,
            added: parsed.added.len(),
            removed_lines: parsed.removed,
            changed_lines: parsed.changed,
            added_lines: parsed.added,
            status: CrateStatus::Ok,
            error: None,
            pubdep: pubdep::compute(ctx, tables, crate_name),
        });
    }

    results
}

fn failure_result(
    ctx: &Ctx,
    crate_name: &str,
    stage: ErrorStage,
    ref_label: &str,
    ref_sha: &str,
    stderr: &str,
) -> CrateResult {
    let stderr = error_tail(stderr);
    let command = error_command(ctx, crate_name, stage);
    let hint = hint(&stderr);

    CrateResult {
        name: crate_name.to_owned(),
        removed: 0,
        changed: 0,
        added: 0,
        removed_lines: Vec::new(),
        changed_lines: Vec::new(),
        added_lines: Vec::new(),
        status: CrateStatus::Error,
        error: Some(ApiError {
            stage,
            ref_label: ref_label.to_owned(),
            ref_sha: ref_sha.to_owned(),
            command,
            stderr,
            hint,
        }),
        pubdep: Vec::new(),
    }
}

fn error_tail(stderr: &str) -> String {
    let cleaned = stderr.replace('\r', "");
    let lines: Vec<_> = cleaned.split_inclusive('\n').collect();
    let tail = lines[lines.len().saturating_sub(80)..]
        .concat()
        .trim_end_matches('\n')
        .to_owned();
    if tail.is_empty() {
        "public API analysis failed without writing stderr".to_owned()
    } else {
        tail
    }
}

fn error_command(ctx: &Ctx, crate_name: &str, stage: ErrorStage) -> String {
    let features = ctx.feature_args.join(" ");
    if stage != ErrorStage::Diff {
        return format!("cargo public-api {features} -p {crate_name} -ss");
    }

    format!(
        "run at {} ({}): cargo public-api {} -p {} -ss; run at {} ({}): cargo public-api {} -p {} -ss",
        ctx.refs.baseline_label,
        ctx.refs.baseline_sha,
        features,
        crate_name,
        ctx.refs.head_label,
        ctx.refs.head_sha,
        features,
        crate_name
    )
}

/// Returns the first matching remediation hint for cargo-public-api stderr.
pub fn hint(stderr: &str) -> String {
    let text = stderr.to_lowercase();
    if text.contains("protoc") || text.contains("protobuf-compiler") {
        "Install protoc, for example brew install protobuf or apt-get install protobuf-compiler, then rerun zc."
    } else if text.contains("custom build command") {
        "A build script failed. Run the command shown above to inspect the crate's build requirements."
    } else if text.contains("cargo.lock")
        || text.contains("lock file")
        || text.contains("lockfile")
    {
        "The lockfile or dependency resolution failed at this ref. Check Cargo.lock and rerun zc."
    } else if text.contains("requires rustc")
        || text
            .find("rustc ")
            .is_some_and(|start| text[start + "rustc ".len()..].contains("is not supported"))
    {
        "The selected Rust toolchain cannot build this ref. Install the required toolchain and rerun zc."
    } else if text.contains("no library targets")
        || text.contains("does not have a library target")
    {
        "cargo-public-api can only analyze library targets. Exclude this crate or add a library target."
    } else if text.contains("could not compile") {
        "The crate did not compile under the selected feature set. Fix the build or choose a supported feature policy."
    } else {
        "Run the command shown above and fix the failing crate build before trusting the API diff."
    }
    .to_owned()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParsedDiff {
    pub(crate) removed: Vec<String>,
    pub(crate) changed: Vec<String>,
    pub(crate) added: Vec<String>,
    pub(crate) changed_old_count: usize,
}

#[derive(Clone, Copy)]
enum DiffSection {
    None,
    Removed,
    Changed,
    Added,
}

pub(crate) fn parse_diff(output: &str) -> ParsedDiff {
    let mut section = DiffSection::None;
    let mut removed = Vec::new();
    let mut changed_old = Vec::new();
    let mut changed_new = Vec::new();
    let mut added = Vec::new();

    for line in output.split('\n') {
        match line {
            "Removed items from the public API" => section = DiffSection::Removed,
            "Changed items in the public API" => section = DiffSection::Changed,
            "Added items to the public API" => section = DiffSection::Added,
            "" | "(none)" => {}
            line if line.starts_with('=') => {}
            line if line.starts_with('-') => match section {
                DiffSection::Removed => removed.push(line[1..].to_owned()),
                DiffSection::Changed => changed_old.push(line[1..].to_owned()),
                DiffSection::None | DiffSection::Added => {}
            },
            line if line.starts_with('+') => match section {
                DiffSection::Changed => changed_new.push(line[1..].to_owned()),
                DiffSection::Added => added.push(line[1..].to_owned()),
                DiffSection::None | DiffSection::Removed => {}
            },
            _ => {}
        }
    }

    let mut changed = Vec::with_capacity(changed_old.len() * 2);
    let mut changed_old_count = 0;
    for (index, old) in changed_old.iter().enumerate() {
        if old.is_empty() {
            continue;
        }
        changed_old_count += 1;
        changed.push(format!("  - {old}"));
        changed.push(format!(
            "  + {}",
            changed_new.get(index).map(String::as_str).unwrap_or("")
        ));
    }

    removed.retain(|line| !line.is_empty());
    added.retain(|line| !line.is_empty());

    ParsedDiff {
        removed,
        changed,
        added,
        changed_old_count,
    }
}

#[cfg(test)]
mod tests;
