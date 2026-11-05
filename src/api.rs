//! Rustdoc JSON builds and per-crate public API analysis.

use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::SystemTime;

use serde_json::Value;

use crate::cache::RunTmp;
use crate::ctx::Ctx;
use crate::model::{ApiError, CrateChange, CrateResult, CrateStatus, ErrorStage};
use crate::pubdep::{self, PubdepTables};

mod compat;

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
    let mut command = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    command.env("RUSTUP_TOOLCHAIN", toolchain);
    if std::env::var_os("RUSTC").is_none() {
        command.arg(format!("+{toolchain}"));
    }
    successful_stdout(command.arg("-vV"))
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
    let lib_name = ctx.lib_name(crate_name, ref_sha)?;
    let cacheable = ctx.api_json_cacheable(ref_sha);
    let cache_name = format!("{}.{}.{}.api.json", ref_sha, ctx.cache.api_fp, crate_name);
    let cache_path = ctx.cache.path(&cache_name);
    if let Some(path) = ctx
        .cache
        .api_json
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&cache_name)
        .cloned()
    {
        return Ok(path);
    }

    if cacheable && valid_cached_json(&cache_path) {
        touch(&cache_path);
        return preserve_json(ctx, &cache_name, &cache_path);
    }

    let feature_args = ctx.features.cargo_args(crate_name, worktree, ref_sha)?;
    let doc_dir = rustdoc_dir(&ctx.toolchain, worktree, target)?;
    let mut command = Command::new("cargo");
    command
        .arg(format!("+{}", ctx.toolchain))
        .args(["rustdoc", "--locked", "-q", "--manifest-path"])
        .arg(worktree.join("Cargo.toml"))
        .args(["-p", crate_name, "--lib"])
        .args(&feature_args)
        .args(["--", "-Z", "unstable-options", "--output-format", "json"])
        .env("CARGO_TARGET_DIR", target)
        .current_dir(worktree);

    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }

    let json_path = doc_dir.join(format!("{lib_name}.json"));
    if !valid_cached_json(&json_path) {
        return Err(format!(
            "rustdoc did not produce valid JSON at {}",
            json_path.display()
        ));
    }

    // Cargo reuses library target names across packages. Never let a later build mutate
    // an artifact that a caller is still reading, even without a writable persistent cache.
    let frozen = preserve_json(ctx, &cache_name, &json_path)?;
    if cacheable {
        ctx.cache.copy_atomic(&cache_name, &frozen);
    }
    Ok(frozen)
}

fn freeze_json(tmp: &RunTmp, source: &Path) -> Result<PathBuf, String> {
    let destination = tmp.sub("api-json")?.join("api.json");
    fs::copy(source, &destination)
        .map_err(|error| format!("cannot preserve rustdoc JSON {}: {error}", source.display()))?;
    Ok(destination)
}
fn preserve_json(ctx: &Ctx, cache_name: &str, source: &Path) -> Result<PathBuf, String> {
    let frozen = freeze_json(&ctx.tmp, source)?;
    ctx.cache
        .api_json
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(cache_name.to_owned(), frozen.clone());
    Ok(frozen)
}

fn rustdoc_dir(toolchain: &str, worktree: &Path, target_dir: &Path) -> Result<PathBuf, String> {
    let output = Command::new("cargo")
        .arg(format!("+{toolchain}"))
        .args(["-Z", "unstable-options", "config", "get", "--format=json"])
        .current_dir(worktree)
        .output()
        .map_err(|error| format!("cannot read Cargo target configuration: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let config: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid Cargo configuration: {error}"))?;
    let target = std::env::var("CARGO_BUILD_TARGET")
        .ok()
        .filter(|value| !value.is_empty())
        .map(Value::String)
        .unwrap_or_else(|| config["build"]["target"].clone());
    configured_doc_dir(target_dir, &target)
}

fn configured_doc_dir(target_dir: &Path, target: &Value) -> Result<PathBuf, String> {
    let target = match target {
        Value::Null => return Ok(target_dir.join("doc")),
        Value::String(target) => target.as_str(),
        Value::Array(targets) if targets.len() == 1 => targets[0]
            .as_str()
            .ok_or_else(|| "Cargo build.target must name a target".to_owned())?,
        Value::Array(_) => {
            return Err(
                "public API analysis requires a single configured Cargo build.target".to_owned(),
            )
        }
        _ => return Err("Cargo build.target must name a target".to_owned()),
    };
    let target_name = Path::new(target)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.strip_suffix(".json").unwrap_or(name))
        .ok_or_else(|| format!("invalid Cargo target {target}"))?;
    Ok(target_dir.join(target_name).join("doc"))
}

fn valid_cached_json(path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    if contents.is_empty() {
        return false;
    }

    serde_json::from_str::<Value>(&contents).is_ok_and(|json| {
        let root = match &json["root"] {
            Value::String(id) => &json["index"][id.as_str()],
            Value::Number(id) => &json["index"][id.to_string()],
            _ => return false,
        };
        json["format_version"]
            .as_u64()
            .is_some_and(|version| version > 0)
            && json["index"].is_object()
            && json["paths"].is_object()
            && json["external_crates"].is_object()
            && root["inner"]["module"]["items"].is_array()
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
    let build = |crate_name: &str, baseline: bool| {
        let (worktree, target, sha, crates) = if baseline {
            (
                &ctx.baseline_worktree,
                &ctx.baseline_target,
                &ctx.refs.baseline_sha,
                &ctx.baseline_crates,
            )
        } else {
            (
                &ctx.head_worktree,
                &ctx.head_target,
                &ctx.refs.head_sha,
                &ctx.head_crates,
            )
        };
        if crates
            .get(crate_name)
            .and_then(|krate| krate.lib_name.as_ref())
            .is_none()
        {
            return Ok(None);
        }
        rustdoc_json(ctx, crate_name, worktree, target, sha).map(Some)
    };
    let (base_jsons, head_jsons) = thread::scope(|scope| {
        let baseline = (ctx.refs.baseline_sha != ctx.refs.head_sha
            || ctx.refs.head_is_worktree_snapshot)
            .then(|| {
                scope.spawn(|| {
                    crates
                        .iter()
                        .map(|name| build(name, true))
                        .collect::<Vec<_>>()
                })
            });
        let head = crates
            .iter()
            .enumerate()
            .map(|(index, name)| {
                ctx.progress.set(&format!(
                    "public-api: [{}/{}] {name}",
                    index + 1,
                    crates.len()
                ));
                build(name, false)
            })
            .collect::<Vec<_>>();
        let base = baseline.map_or_else(
            || head.clone(),
            |worker| {
                worker
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            },
        );
        (base, head)
    });

    let mut results = Vec::with_capacity(crates.len());
    for ((crate_name, base), head) in crates.iter().zip(base_jsons).zip(head_jsons) {
        let base = match base {
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
        let head = match head {
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
        let (change, lib_name) = crate_identity(ctx, crate_name);
        let mut command = Command::new("cargo");
        command
            .arg("public-api")
            .args(["-p", crate_name, "-ss", "--color=never"])
            .current_dir(&ctx.tmp.dir);
        match (&base, &head) {
            (Some(base), Some(head)) => {
                command.arg("diff").arg(base).arg(head);
            }
            (Some(path), None) | (None, Some(path)) => {
                command.arg("--rustdoc-json").arg(path);
            }
            (None, None) => {
                results.push(diff_failure(
                    ctx,
                    crate_name,
                    &format!("package {crate_name} has no library at either ref"),
                ));
                continue;
            }
        }

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

        let breaking_added = if let (Some(base_json), Some(head_json)) = (&base, &head) {
            match compat::breaking_additions(base_json, head_json) {
                Ok(additions) => additions,
                Err(stderr) => {
                    results.push(diff_failure(ctx, crate_name, &stderr));
                    continue;
                }
            }
        } else {
            Vec::new()
        };
        let pubdep = if let Some(head_json) = &head {
            match pubdep::compute_from_json(ctx, tables, crate_name, base.as_deref(), head_json) {
                Ok(findings) => findings,
                Err(stderr) => {
                    results.push(diff_failure(ctx, crate_name, &stderr));
                    continue;
                }
            }
        } else {
            Vec::new()
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed = if change == CrateChange::Existing {
            parse_diff(&stdout)
        } else {
            one_sided_diff(&stdout, change)
        };
        results.push(CrateResult {
            name: crate_name.clone(),
            lib_name,
            change,
            breaking_added,
            removed: parsed.removed.len(),
            changed: parsed.changed_old_count,
            added: parsed.added.len(),
            removed_lines: parsed.removed,
            changed_lines: parsed.changed,
            added_lines: parsed.added,
            status: CrateStatus::Ok,
            error: None,
            pubdep,
        });
    }

    results
}

fn one_sided_diff(output: &str, change: CrateChange) -> ParsedDiff {
    let lines = output
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    let (removed, added) = if change == CrateChange::Removed {
        (lines, Vec::new())
    } else {
        (Vec::new(), lines)
    };
    ParsedDiff {
        removed,
        changed: Vec::new(),
        added,
        changed_old_count: 0,
    }
}

fn crate_identity(ctx: &Ctx, crate_name: &str) -> (CrateChange, String) {
    let base = ctx
        .baseline_crates
        .get(crate_name)
        .and_then(|krate| krate.lib_name.as_deref());
    let head = ctx
        .head_crates
        .get(crate_name)
        .and_then(|krate| krate.lib_name.as_deref());
    library_identity(crate_name, base, head)
}

fn library_identity(
    crate_name: &str,
    base: Option<&str>,
    head: Option<&str>,
) -> (CrateChange, String) {
    match (base, head) {
        (Some(_), Some(head)) => (CrateChange::Existing, head.to_owned()),
        (None, Some(head)) => (CrateChange::Added, head.to_owned()),
        (Some(base), None) => (CrateChange::Removed, base.to_owned()),
        (None, None) => (CrateChange::Existing, crate_name.to_owned()),
    }
}

fn diff_failure(ctx: &Ctx, crate_name: &str, stderr: &str) -> CrateResult {
    failure_result(
        ctx,
        crate_name,
        ErrorStage::Diff,
        &format!("{}..{}", ctx.refs.baseline_label, ctx.refs.head_label),
        &format!("{}..{}", ctx.refs.baseline_sha, ctx.refs.head_sha),
        stderr,
    )
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
    let (change, lib_name) = crate_identity(ctx, crate_name);

    CrateResult {
        name: crate_name.to_owned(),
        lib_name,
        change,
        breaking_added: Vec::new(),
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
    let command_at = |worktree: &Path, ref_sha: &str| {
        let mut parts = vec![format!("cargo +{} rustdoc --locked --lib", ctx.toolchain)];
        parts.extend(
            ctx.features
                .cargo_args(crate_name, worktree, ref_sha)
                .unwrap_or_default(),
        );
        parts.push(format!(
            "-p {crate_name} -- -Z unstable-options --output-format json"
        ));
        format!("cd {} && {}", worktree.display(), parts.join(" "))
    };
    let baseline = command_at(&ctx.baseline_worktree, &ctx.refs.baseline_sha);
    let head = command_at(&ctx.head_worktree, &ctx.refs.head_sha);
    match stage {
        ErrorStage::BaselineBuild => baseline,
        ErrorStage::HeadBuild => head,
        ErrorStage::Diff => format!(
            "{baseline}; {head}; cargo public-api -p {crate_name} -ss diff <baseline-json> <head-json>; compatibility/public-dependency analysis",
        ),
    }
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
        "The crate did not compile under the selected feature set. Fix the build, or choose a feature policy it builds under with --features."
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
