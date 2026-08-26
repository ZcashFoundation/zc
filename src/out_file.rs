//! Document file output (`--report`, `--changelog-file`).
//!
//! `label` names the document in the diagnostics, so each destination fails in its own
//! words.

use std::fs;
use std::path::{Path, PathBuf};

/// Directory the document is written into; a bare file name means the current directory.
fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Reject a destination whose directory is missing, before the analysis runs.
pub fn check_dir(label: &str, path: &Path) -> Result<(), String> {
    let dir = parent_dir(path);
    if dir.is_dir() {
        Ok(())
    } else {
        Err(format!(
            "{label} directory '{}' does not exist",
            dir.display()
        ))
    }
}

pub fn write(label: &str, path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents)
        .map_err(|err| format!("failed to write {label} '{}': {err}", path.display()))
}

#[cfg(test)]
#[path = "out_file/tests.rs"]
mod tests;
