//! JSON report file output (`--report`).

use std::fs;
use std::path::{Path, PathBuf};

/// Directory the report is written into; a bare file name means the current directory.
fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Reject a destination whose directory is missing, before the analysis runs.
pub fn check_dir(path: &Path) -> Result<(), String> {
    let dir = parent_dir(path);
    if dir.is_dir() {
        Ok(())
    } else {
        Err(format!(
            "report directory '{}' does not exist",
            dir.display()
        ))
    }
}

pub fn write(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents)
        .map_err(|err| format!("failed to write report '{}': {err}", path.display()))
}

#[cfg(test)]
#[path = "report_file/tests.rs"]
mod tests;
