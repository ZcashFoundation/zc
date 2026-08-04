//! Git subprocess operations.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Runs Git and returns trimmed stdout.
pub fn git(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|err| format!("failed to run git: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err("git command failed".to_string())
        } else {
            Err(stderr)
        }
    }
}

/// Runs Git with all output discarded.
pub fn git_quiet(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Reports whether a ref resolves to an object.
pub fn rev_parse_ok(r: &str) -> bool {
    git_quiet(&["rev-parse", "--verify", "--quiet", r])
}

/// Resolves a ref to its full object ID.
pub fn rev_parse_verify(r: &str) -> Result<String, String> {
    git(&["rev-parse", "--verify", r])
}

/// Resolves a ref to a short object ID, retaining the input on failure.
pub fn rev_parse_short(r: &str) -> String {
    git(&["rev-parse", "--short", r]).unwrap_or_else(|_| r.to_string())
}

/// Finds the best common ancestor of two refs.
pub fn merge_base(a: &str, b: &str) -> Option<String> {
    git(&["merge-base", a, b])
        .ok()
        .filter(|sha| !sha.is_empty())
}

/// Chooses the current branch's useful parent, falling back to `main`.
pub fn detect_parent_branch() -> String {
    let current = git(&["branch", "--show-current"]).unwrap_or_default();
    if current.is_empty() || current == "main" {
        return "main".to_string();
    }

    let mut upstream = git(&[
        "rev-parse",
        "--abbrev-ref",
        &format!("{current}@{{upstream}}"),
    ])
    .unwrap_or_default();
    if upstream
        .split_once('/')
        .map_or(upstream == current, |(_, name)| name == current)
    {
        upstream.clear();
    }

    if !upstream.is_empty() && rev_parse_ok(&upstream) {
        return upstream;
    }

    if !upstream.is_empty() {
        let local = upstream
            .strip_prefix("origin/")
            .unwrap_or(upstream.as_str());
        if rev_parse_ok(local) {
            return local.to_string();
        }
    }

    "main".to_string()
}

/// Reports tracked, staged, or untracked non-ignored changes.
pub fn is_worktree_dirty() -> bool {
    git(&["status", "--porcelain"]).is_ok_and(|out| !out.is_empty())
}

/// Creates an unreachable commit containing the working tree without changing the real index.
pub fn worktree_snapshot_commit(run_tmp: &Path) -> Result<String, String> {
    let tmp_index = unique_file(run_tmp, ".index")
        .map_err(|_| "failed to create temporary git index".to_string())?;
    let _cleanup = RemoveFile(tmp_index.clone());

    let head_sha = git(&["rev-parse", "--verify", "HEAD"])
        .map_err(|_| "cannot resolve HEAD (no commits yet?)".to_string())?;

    if !git_with_index(&tmp_index, &["read-tree", &head_sha], true).0 {
        return Err("git read-tree HEAD failed".to_string());
    }
    if !git_with_index(&tmp_index, &["add", "-A"], true).0 {
        return Err("git add -A failed (working tree snapshot)".to_string());
    }

    let (ok, tree) = git_with_index(&tmp_index, &["write-tree"], false);
    if !ok {
        return Err("git write-tree failed".to_string());
    }
    let tree = tree.trim();

    let output = Command::new("git")
        .args([
            "commit-tree",
            tree,
            "-p",
            &head_sha,
            "-m",
            "[zc worktree snapshot]",
        ])
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "git commit-tree failed".to_string())?;
    if !output.status.success() {
        return Err("git commit-tree failed".to_string());
    }
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if commit.is_empty() {
        Err("git commit-tree failed".to_string())
    } else {
        Ok(commit)
    }
}

/// Adds a quiet detached worktree at a resolved commit.
pub fn worktree_add(dir: &Path, sha: &str) -> Result<(), String> {
    let status = Command::new("git")
        .args(["worktree", "add", "--detach", "--quiet"])
        .arg(dir)
        .arg(sha)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| format!("failed to run git worktree add: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("git worktree add failed".to_string())
    }
}

/// Forcibly removes a registered worktree, ignoring failures.
pub fn worktree_remove(dir: &Path) {
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Prunes stale worktree registrations.
pub fn worktree_prune() {
    let _ = Command::new("git")
        .args(["worktree", "prune"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Reads one file from a Git tree.
pub fn show_file(rev: &str, path: &str) -> Option<String> {
    let spec = format!("{rev}:{path}");
    let output = Command::new("git").args(["show", &spec]).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Maps source-level public type names to their declaration keywords.
pub fn type_kinds(head_sha: &str) -> HashMap<String, String> {
    let output = match Command::new("git")
        .args([
            "grep",
            "-E",
            "^[[:space:]]*pub (struct|enum|trait|union) ",
            head_sha,
            "--",
            "*.rs",
        ])
        .stderr(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return HashMap::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut pairs: Vec<(String, String)> = text.lines().filter_map(parse_type_kind_line).collect();
    pairs.sort_unstable();
    pairs.dedup();

    let mut kinds = HashMap::new();
    for (name, kind) in pairs {
        kinds.insert(name, kind);
    }
    kinds
}

fn parse_type_kind_line(line: &str) -> Option<(String, String)> {
    let mut words = line.split_whitespace();
    while let Some(word) = words.next() {
        if matches!(word, "struct" | "enum" | "trait" | "union") {
            let raw_name = words.next()?;
            let end = raw_name
                .find(['<', '(', '{', ':', ';'])
                .unwrap_or(raw_name.len());
            let name = &raw_name[..end];
            return (!name.is_empty()).then(|| (name.to_string(), word.to_string()));
        }
    }
    None
}

fn git_with_index(index: &Path, args: &[&str], discard_stderr: bool) -> (bool, String) {
    let mut command = Command::new("git");
    command.args(args).env("GIT_INDEX_FILE", index);
    if discard_stderr {
        command.stderr(Stdio::null());
    }
    match command.output() {
        Ok(output) => (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ),
        Err(_) => (false, String::new()),
    }
}

fn unique_file(parent: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    for _ in 0..1_000 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{prefix}.{}.{sequence}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique file name",
    ))
}

struct RemoveFile(PathBuf);

impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
#[path = "git/tests.rs"]
mod tests;
