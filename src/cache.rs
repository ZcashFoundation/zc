//! Persistent analysis cache and per-run temporary directories.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sha1::{Digest, Sha1};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const API_JSON_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

pub struct Cache {
    pub dir: PathBuf,
    pub script_hash: String,
    pub api_fp: String,
}

impl Cache {
    pub fn new(
        version: &str,
        cargo_public_api_version: &str,
        rustc_version: &str,
        feature_args: &[String],
    ) -> Result<Cache, String> {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| OsString::from("target"));
        let cache_dir = PathBuf::from(target).join("zc-cache");
        fs::create_dir_all(&cache_dir)
            .map_err(|_| "failed to resolve zc cache directory".to_string())?;
        let dir = fs::canonicalize(cache_dir)
            .map_err(|_| "failed to resolve zc cache directory".to_string())?;

        let script_hash = std::env::current_exe()
            .ok()
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| sha1_short(&bytes))
            .unwrap_or_else(|| format!("v{version}"));

        let mut fingerprint = Vec::new();
        append_fingerprint_line(&mut fingerprint, cargo_public_api_version);
        append_fingerprint_line(&mut fingerprint, rustc_version);
        if feature_args.is_empty() {
            append_fingerprint_line(&mut fingerprint, "");
        }
        for arg in feature_args {
            append_fingerprint_line(&mut fingerprint, arg);
        }
        let api_fp = sha1_short(&fingerprint);

        Ok(Cache {
            dir,
            script_hash,
            api_fp,
        })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn read_if_present(&self, name: &str) -> Option<String> {
        let contents = fs::read_to_string(self.path(name)).ok()?;
        (!contents.is_empty()).then_some(contents)
    }

    pub fn write_atomic(&self, name: &str, contents: &str) {
        let Ok((tmp, mut file)) = unique_file(&self.dir, ".zc-cache-write") else {
            return;
        };
        let wrote = file.write_all(contents.as_bytes()).is_ok();
        drop(file);
        if !wrote || fs::rename(&tmp, self.path(name)).is_err() {
            let _ = fs::remove_file(tmp);
        }
    }

    pub fn copy_atomic(&self, name: &str, src: &Path) {
        let Ok((tmp, file)) = unique_file(&self.dir, ".zc-cache-copy") else {
            return;
        };
        drop(file);
        if fs::copy(src, &tmp).is_err() || fs::rename(&tmp, self.path(name)).is_err() {
            let _ = fs::remove_file(tmp);
        }
    }

    pub fn prune_old_api_json(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_api_json = entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".api.json"));
            if !is_api_json {
                continue;
            }
            let is_old = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > API_JSON_MAX_AGE);
            if is_old {
                let _ = fs::remove_file(path);
            }
        }
    }
}

pub struct RunTmp {
    pub dir: PathBuf,
}

impl RunTmp {
    pub fn new() -> Result<RunTmp, String> {
        unique_dir(&std::env::temp_dir(), "zc")
            .map(|dir| RunTmp { dir })
            .map_err(|err| format!("failed to create zc run temp directory: {err}"))
    }

    pub fn sub(&self, prefix: &str) -> Result<PathBuf, String> {
        unique_dir(&self.dir, prefix)
            .map_err(|err| format!("failed to create zc temporary directory: {err}"))
    }
}

impl Drop for RunTmp {
    fn drop(&mut self) {
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    crate::git::worktree_remove(&entry.path());
                }
            }
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub fn sha1_short(bytes: &[u8]) -> String {
    let digest = Sha1::digest(bytes);
    hex::encode(digest)[..12].to_string()
}

fn append_fingerprint_line(fingerprint: &mut Vec<u8>, value: &str) {
    fingerprint.extend_from_slice(value.as_bytes());
    fingerprint.push(b'\n');
}

fn unique_dir(parent: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    for _ in 0..1_000 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{prefix}.{}.{sequence}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique directory name",
    ))
}

fn unique_file(parent: &Path, prefix: &str) -> std::io::Result<(PathBuf, File)> {
    for _ in 0..1_000 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{prefix}.{}.{sequence}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique file name",
    ))
}

#[cfg(test)]
#[path = "cache/tests.rs"]
mod tests;
