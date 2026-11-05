//! Run context: everything resolved once at startup and read by every analysis module.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::cache::{Cache, RunTmp};
use crate::cargo_meta::WorkspaceCrate;
use crate::features::CrateFeatures;
use crate::model::{Options, Refs};
use crate::progress::Progress;
use crate::style::Style;

/// Shared, immutable-after-setup run state.
pub struct Ctx {
    pub opts: Options,
    pub style: Style,
    pub cache: Cache,
    pub tmp: RunTmp,
    /// Nightly toolchain name used for rustdoc JSON.
    pub toolchain: String,
    /// Feature arguments for every rustdoc JSON build and cargo-public-api run.
    pub features: CrateFeatures,
    pub refs: Refs,
    pub progress: Progress,
    /// Detached worktree at the baseline ref.
    pub baseline_worktree: PathBuf,
    /// Detached worktree at the head ref.
    pub head_worktree: PathBuf,
    pub baseline_crates: BTreeMap<String, WorkspaceCrate>,
    pub head_crates: BTreeMap<String, WorkspaceCrate>,
    /// `CARGO_TARGET_DIR` for baseline builds.
    pub baseline_target: PathBuf,
    /// `CARGO_TARGET_DIR` for head builds.
    pub head_target: PathBuf,
}

impl Ctx {
    /// Returns the actual Cargo library target, including custom names and proc macros.
    pub fn lib_name(&self, crate_name: &str, ref_sha: &str) -> Result<&str, String> {
        let crates = if ref_sha == self.refs.baseline_sha {
            &self.baseline_crates
        } else if ref_sha == self.refs.head_sha {
            &self.head_crates
        } else {
            return Err(format!("no workspace metadata for ref {ref_sha}"));
        };
        crates
            .get(crate_name)
            .and_then(|krate| krate.lib_name.as_deref())
            .ok_or_else(|| format!("package {crate_name} has no library target at {ref_sha}"))
    }

    /// True when a rustdoc JSON result for this ref may be cached: a working-tree snapshot
    /// is not content-addressed by its SHA in any stable way, so it never is.
    pub fn api_json_cacheable(&self, ref_sha: &str) -> bool {
        !self.refs.head_is_worktree_snapshot || ref_sha != self.refs.head_sha
    }
}
