//! Shared data model. Every module in this crate speaks these types; nothing here shells out.

use std::fmt;
use std::path::PathBuf;

/// Exit codes, mirroring the documented contract.
pub const EXIT_OK: i32 = 0;
pub const EXIT_BREAKING: i32 = 1;
pub const EXIT_ANALYSIS: i32 = 2;
pub const EXIT_USAGE: i32 = 64;

/// Crate names whose dependencies are excluded from workspace dep classification: test-only
/// crates whose "runtime" deps are really downstream test deps.
pub fn is_test_crate(name: &str) -> bool {
    name == "zebra-test" || name.ends_with("-test")
}

/// Per-crate item grouping for the detailed diff sections.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupMode {
    /// Cluster by owning module, with type sub-headers (default).
    Mod,
    /// One tagged header per type; members keep their `Type::` prefix.
    Type,
    /// Ungrouped, fully-qualified, one item per line.
    Flat,
}

/// Command-line options.
#[derive(Clone, Debug)]
pub struct Options {
    pub with_lock: bool,
    pub with_values: bool,
    pub json_mode: bool,
    pub changelog_mode: bool,
    pub group_mode: GroupMode,
    /// Path the JSON report is written to, in addition to the selected stdout output.
    pub report_path: Option<PathBuf>,
}

/// The two ends of the comparison, fully resolved.
#[derive(Clone, Debug)]
pub struct Refs {
    /// Baseline ref as resolved (may be a merge-base SHA).
    pub baseline: String,
    /// Display label, e.g. `merge-base(main, HEAD)`.
    pub baseline_label: String,
    pub baseline_sha: String,
    pub baseline_short: String,
    /// Head ref; a synthesized snapshot commit when the working tree is dirty.
    pub head_ref: String,
    /// Display label, e.g. `working tree`.
    pub head_label: String,
    pub head_sha: String,
    pub head_short: String,
    pub head_is_worktree_snapshot: bool,
}

/// Strongest dependency kind a crate is used with across the workspace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepKind {
    Runtime,
    Build,
    Dev,
    Unused,
}

impl DepKind {
    /// Rank for "strongest kind wins" comparisons.
    pub fn rank(self) -> u8 {
        match self {
            DepKind::Runtime => 3,
            DepKind::Build => 2,
            DepKind::Dev => 1,
            DepKind::Unused => 0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DepKind::Runtime => "runtime",
            DepKind::Build => "build",
            DepKind::Dev => "dev",
            DepKind::Unused => "unused",
        }
    }
}

impl fmt::Display for DepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A kind label including optionality: `runtime-opt` for an optional runtime dep.
pub fn kind_label(kind: DepKind, optional: bool) -> String {
    if optional && kind == DepKind::Runtime {
        "runtime-opt".to_string()
    } else {
        kind.as_str().to_string()
    }
}

/// Rank of a rendered kind label, so old/new labels can be compared directly.
pub fn label_rank(label: &str) -> u8 {
    match label {
        "runtime" | "runtime-opt" => 3,
        "build" => 2,
        "dev" => 1,
        _ => 0,
    }
}

/// One workspace dependency at one ref, keyed elsewhere by its display name
/// (the Cargo rename if any, formatted `foo (pkg: bar)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepRecord {
    /// Version requirement from Cargo.toml, leading `^` stripped.
    pub ver: String,
    pub kind: DepKind,
    /// True when every max-kind usage is optional.
    pub optional: bool,
    /// True when any usage enables default features.
    pub default_features: bool,
    /// Sorted union of explicitly-enabled features.
    pub features: Vec<String>,
}

/// Semver-compatibility class of a requirement change, under Cargo's caret rules.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bump {
    Major,
    Minor,
    Patch,
    /// The requirement changed without moving the version it starts at, so the text alone
    /// cannot say whether a consumer is affected.
    Unknown,
}

impl Bump {
    pub fn as_str(self) -> &'static str {
        match self {
            Bump::Major => "major",
            Bump::Minor => "minor",
            Bump::Patch => "patch",
            Bump::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Bump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct DepRemoved {
    pub name: String,
    pub version: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct DepChanged {
    pub name: String,
    pub old: String,
    pub new: String,
    pub bump: Bump,
    pub kind: String,
    /// Rendered feature delta, e.g. `-default!,+std,-foo` (empty when unchanged).
    pub features: String,
}

#[derive(Clone, Debug)]
pub struct DepAdded {
    pub name: String,
    pub version: String,
    pub kind: String,
}

/// Workspace dependency diff plus its breaking count.
#[derive(Clone, Debug, Default)]
pub struct DepDiff {
    pub removed: Vec<DepRemoved>,
    pub changed: Vec<DepChanged>,
    pub added: Vec<DepAdded>,
    /// Only runtime (non-optional) deps that are removed, major-bumped, or lose features.
    pub breaking: usize,
}

/// Scope of a per-crate dependency row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// The dep is itself a workspace member.
    Internal,
    External,
    /// Pseudo-row carrying the crate's `rust-version`.
    Msrv,
}

/// One (workspace crate, direct runtime/build dependency) pair at a ref.
#[derive(Clone, Debug)]
pub struct PerCrateDep {
    pub crate_name: String,
    /// Display key: the Cargo rename if any, else the real crate name.
    pub dep: String,
    pub req: String,
    pub scope: Scope,
    /// Real package name (rustdoc renders foreign paths by this).
    pub pkg: String,
}

/// Transitive (Cargo.lock) diff.
#[derive(Clone, Debug, Default)]
pub struct LockDiff {
    /// `(name, versions)` — versions comma-joined when a crate is locked at several majors.
    pub removed: Vec<(String, String)>,
    /// `(name, old versions, new versions)`.
    pub changed: Vec<(String, String, String)>,
    pub added: Vec<(String, String)>,
    /// Reverse attribution: transitive crate -> direct deps pulling it in, already
    /// truncated to three with `...(+N)`.
    pub via: std::collections::HashMap<String, String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrateStatus {
    Ok,
    Error,
}

/// Stage at which per-crate analysis failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorStage {
    BaselineBuild,
    HeadBuild,
    Diff,
}

impl ErrorStage {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorStage::BaselineBuild => "baseline_build",
            ErrorStage::HeadBuild => "head_build",
            ErrorStage::Diff => "diff",
        }
    }
}

/// A per-crate analysis failure, as surfaced in the human report and in `--json`.
#[derive(Clone, Debug)]
pub struct ApiError {
    pub stage: ErrorStage,
    /// Ref label, or `base..head` for the diff stage.
    pub ref_label: String,
    pub ref_sha: String,
    pub command: String,
    /// Last 80 lines of stderr.
    pub stderr: String,
    pub hint: String,
}

/// A dependency that both changed incompatibly (or unclearly) and is reachable in the
/// crate's public API.
#[derive(Clone, Debug)]
pub struct PubdepFinding {
    pub dep: String,
    pub old: String,
    pub new: String,
}

/// Everything known about one workspace crate after the API diff.
#[derive(Clone, Debug)]
pub struct CrateResult {
    pub name: String,
    pub removed: usize,
    pub changed: usize,
    pub added: usize,
    /// Bare signature lines.
    pub removed_lines: Vec<String>,
    /// Alternating `  - <old>` / `  + <new>` lines.
    pub changed_lines: Vec<String>,
    pub added_lines: Vec<String>,
    pub status: CrateStatus,
    pub error: Option<ApiError>,
    pub pubdep: Vec<PubdepFinding>,
}

impl CrateResult {
    pub fn total(&self) -> usize {
        self.removed + self.changed + self.added
    }

    /// The crate's lib path prefix, e.g. `zebra-state` -> `zebra_state`.
    pub fn prefix(&self) -> String {
        self.name.replace('-', "_")
    }
}

/// A `pub const`/`pub static` whose evaluated value changed.
#[derive(Clone, Debug)]
pub struct ValueChange {
    pub crate_name: String,
    pub path: String,
    pub ty: String,
    pub old: String,
    pub new: String,
}

/// A public item whose doc text changed.
#[derive(Clone, Debug)]
pub struct DocChange {
    pub crate_name: String,
    pub path: String,
}

/// One record in the grouped item stream consumed by the human renderer.
/// Mirrors the `H`/`M`/`T`/`I`/`J`/`X` records the previous awk emitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupRecord {
    /// Flat `--by-type` header: type path + resolved kind.
    TypeHeader { name: String, kind: String },
    /// Nested-mode module header.
    ModHeader(String),
    /// Nested-mode type sub-header (name relative to its module) + resolved kind.
    TypeSub { name: String, kind: String },
    /// Item directly under a `TypeHeader`/`ModHeader`.
    Item(String),
    /// Item under a `TypeSub` (deeper indent).
    DeepItem(String),
    /// Divider introducing trait impls on external types.
    ExtDivider,
}

/// Which diff bucket a section renders.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Removed,
    Changed,
    Added,
}

/// Overall verdict.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Ok,
    Breaking,
    Error,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::Breaking => "breaking",
            Verdict::Error => "error",
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Verdict::Ok => EXIT_OK,
            Verdict::Breaking => EXIT_BREAKING,
            Verdict::Error => EXIT_ANALYSIS,
        }
    }
}

/// Aggregated analysis state, assembled by `main` and consumed by the renderers.
#[derive(Clone, Debug)]
pub struct Report {
    pub refs: Refs,
    pub deps: DepDiff,
    pub crates: Vec<CrateResult>,
    pub crate_count: usize,
    pub values: Vec<ValueChange>,
    pub docs: Vec<DocChange>,
    pub removed_total: usize,
    pub changed_total: usize,
    pub added_total: usize,
    pub changed_crate_count: usize,
    pub error_crate_count: usize,
    pub pubdep_break_total: usize,
    pub pubdep_review_total: usize,
}

impl Report {
    pub fn api_breaking(&self) -> usize {
        self.removed_total + self.changed_total
    }

    pub fn any_breaking(&self) -> bool {
        self.api_breaking() > 0
            || self.deps.breaking > 0
            || !self.values.is_empty()
            || self.pubdep_break_total > 0
    }

    pub fn verdict(&self) -> Verdict {
        if self.error_crate_count > 0 {
            Verdict::Error
        } else if self.any_breaking() {
            Verdict::Breaking
        } else {
            Verdict::Ok
        }
    }
}
