//! Document file output (`--report`, `--changelog-file`).
//!
//! `label` names the document in the diagnostics, so each destination fails in its own
//! words.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Directory the document is written into; a bare file name means the current directory.
fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Reject a destination whose directory is missing and clear a document left by an earlier
/// run, both before the analysis starts.
///
/// Clearing is what makes the destination mean "this run": a run that produces no document —
/// an analysis error yields no changelog draft — would otherwise leave the previous run's file
/// in place for a consumer to read as current, and `--fail-on none` makes that run exit 0.
pub fn prepare(label: &str, path: &Path) -> Result<(), String> {
    let dir = parent_dir(path);
    if !dir.is_dir() {
        return Err(format!(
            "{label} directory '{}' does not exist",
            dir.display()
        ));
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed to clear the previous {label} '{}': {err}",
            path.display()
        )),
    }
}

pub fn write(label: &str, path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents)
        .map_err(|err| format!("failed to write {label} '{}': {err}", path.display()))
}

#[cfg(test)]
#[path = "out_file/tests.rs"]
mod tests;
