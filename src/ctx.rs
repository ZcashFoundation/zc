//! Run context: everything resolved once at startup and read by every analysis module.

use std::path::PathBuf;

use crate::cache::{Cache, RunTmp};
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
    /// Feature policy passed to every cargo-public-api and rustdoc invocation.
    pub feature_args: Vec<String>,
    pub refs: Refs,
    pub progress: Progress,
    /// Detached worktree at the baseline ref.
    pub baseline_worktree: PathBuf,
    /// Detached worktree at the head ref.
    pub head_worktree: PathBuf,
    /// `CARGO_TARGET_DIR` for baseline builds.
    pub baseline_target: PathBuf,
    /// `CARGO_TARGET_DIR` for head builds.
    pub head_target: PathBuf,
}

impl Ctx {
    /// True when a rustdoc JSON result for this ref may be cached: a working-tree snapshot
    /// is not content-addressed by its SHA in any stable way, so it never is.
    pub fn api_json_cacheable(&self, ref_sha: &str) -> bool {
        !self.refs.head_is_worktree_snapshot || ref_sha != self.refs.head_sha
    }
}
