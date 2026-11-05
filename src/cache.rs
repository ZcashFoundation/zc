//! Persistent analysis cache and per-run temporary directories.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use sha1::{Digest, Sha1};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// Bump whenever the rustdoc invocation or artifact interpretation changes.
const API_CACHE_SCHEMA: &str = "zc-rustdoc-json-v3";
const API_JSON_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

pub struct Cache {
    pub dir: PathBuf,
    pub script_hash: String,
    pub api_fp: String,
    pub(crate) api_json: Mutex<BTreeMap<String, PathBuf>>,
}

impl Cache {
    pub fn new(
        version: &str,
        cargo_public_api_version: &str,
        rustc_version: &str,
        feature_args: &[String],
    ) -> Result<Cache, String> {
        let target = match std::env::var_os("CARGO_TARGET_DIR").filter(|value| !value.is_empty()) {
            Some(target) => PathBuf::from(target),
            None => {
                PathBuf::from(crate::git::git(&["rev-parse", "--show-toplevel"])?).join("target")
            }
        };
        let cache_dir = target.join("zc-cache");
        fs::create_dir_all(&cache_dir)
            .map_err(|_| "failed to resolve zc cache directory".to_string())?;
        let dir = fs::canonicalize(cache_dir)
            .map_err(|_| "failed to resolve zc cache directory".to_string())?;

        let script_hash = std::env::current_exe()
            .ok()
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| sha1_short(&bytes))
            .unwrap_or_else(|| format!("v{version}"));

        let api_fp = api_fingerprint(
            cargo_public_api_version,
            rustc_version,
            feature_args,
            &std::env::vars_os().collect(),
        );

        Ok(Cache {
            dir,
            script_hash,
            api_fp,
            api_json: Mutex::new(BTreeMap::new()),
        })
    }

    /// Adds only configuration outside the compared commits to the shared fingerprint.
    /// Ref-local `.cargo` configuration is already identified by the commit SHA.
    pub fn configure_api(&mut self, toolchain: &str, worktrees: &[&Path]) -> Result<(), String> {
        let mut files = BTreeSet::new();
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
        if let Some(home) = cargo_home {
            files.insert(home.join("config"));
            files.insert(home.join("config.toml"));
        }
        for worktree in worktrees {
            for ancestor in worktree.ancestors().skip(1) {
                files.insert(ancestor.join(".cargo/config"));
                files.insert(ancestor.join(".cargo/config.toml"));
            }
        }
        let mut fingerprint = Vec::new();
        append_fingerprint_line(&mut fingerprint, &self.api_fp);
        append_fingerprint_line(&mut fingerprint, toolchain);
        for file in files {
            match fs::read(&file) {
                Ok(bytes) => {
                    append_fingerprint_bytes(&mut fingerprint, file.as_os_str().as_bytes());
                    append_fingerprint_bytes(&mut fingerprint, &bytes);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("cannot read {}: {error}", file.display())),
            }
        }
        self.api_fp = sha1_short(&fingerprint);
        Ok(())
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

fn api_fingerprint(
    cargo_public_api_version: &str,
    rustc_version: &str,
    feature_args: &[String],
    environment: &BTreeMap<OsString, OsString>,
) -> String {
    let mut fingerprint = Vec::new();
    append_fingerprint_line(&mut fingerprint, API_CACHE_SCHEMA);
    append_fingerprint_line(&mut fingerprint, cargo_public_api_version);
    append_fingerprint_line(&mut fingerprint, rustc_version);
    fingerprint.extend_from_slice(&(feature_args.len() as u64).to_le_bytes());
    for arg in feature_args {
        append_fingerprint_line(&mut fingerprint, arg);
    }
    // Keep encoded flags as raw bytes: Cargo uses a unit separator, not shell splitting.
    for (name, value) in environment {
        if name.as_bytes().starts_with(b"CARGO_")
            || name.as_bytes().starts_with(b"RUST")
            || name.as_bytes() == b"PATH"
        {
            append_fingerprint_bytes(&mut fingerprint, name.as_bytes());
            append_fingerprint_bytes(&mut fingerprint, value.as_bytes());
        }
    }
    sha1_short(&fingerprint)
}

fn append_fingerprint_line(fingerprint: &mut Vec<u8>, value: &str) {
    append_fingerprint_bytes(fingerprint, value.as_bytes());
}

fn append_fingerprint_bytes(fingerprint: &mut Vec<u8>, value: &[u8]) {
    fingerprint.extend_from_slice(&(value.len() as u64).to_le_bytes());
    fingerprint.extend_from_slice(value);
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
