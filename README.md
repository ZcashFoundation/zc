# zc

Diff the **public API** and **dependencies** of a Rust workspace's crates
between two git refs — and flag the changes that are breaking for downstream
consumers.

`zc` wraps [`cargo public-api`](https://github.com/cargo-public-api/cargo-public-api)
and adds workspace-wide dependency, lockfile, and const/value diffing on top, so
a single command tells you whether a branch breaks your published surface.

## What it reports

1. **Workspace dependency diff** — each dep classified by its strongest use
   (runtime > build > dev). Requirements are compared per member, target, and kind
   before aggregation; bounds and ambiguous changes are review items, not invented major bumps.
2. **Transitive `Cargo.lock` diff** (`--with-lock`) — transitive version changes,
   annotated with the direct deps that pull them in and retained in machine reports.
3. **Per-crate public-API diff** (`cargo public-api`) — removed / changed / added
   items across the union of baseline and head libraries. Added and removed crates
   are reported explicitly. Binary-only members are listed as skipped; unpublished
   libraries remain analyzed because path and Git dependencies can consume them.
   Builds use `--all-features` by default; `--features` narrows that policy.
   Common breaking additions are detected: exhaustive enum variants, loss of struct or
   enum-variant construction, and required methods on implementable public traits.
   Analysis failures retain their stage, command, stderr, and hint.
4. **Values and documentation** (`--with-values`) — const/static and associated-constant
   changes, including inherited defaults, plus documentation changes on public items
   and implicit enum/trait members. Adding the first or removing the last doc comment counts.
   Conflicting evidence for one public value fails analysis instead of guessing.
   Surviving implementation aliases are compared beyond the selected display path.
5. **Public-dependency breaks** — joins changed runtime dependencies with foreign
   types exposed at both revisions, including re-exports, trait implementations,
   and custom library names. Public package replacements count as type-identity
   breaks even when the alias and requirement stay unchanged. Ambiguous crate identities
   produce an analysis error instead of an unsupported verdict.
   Unchanged dependencies are checked only when they can affect a changed pair.

It ends with a `BREAKING` / `ERROR` / `OK` verdict. `--json` emits machine-readable output for CI and agents.

`zc` materializes both refs in disposable detached worktrees and runs Cargo there
with `--locked`. Each ref uses its own Cargo configuration and target directory;
per-package JSON is preserved before later builds reuse Cargo output paths.
The invoking checkout's HEAD, index, source, and `Cargo.lock` remain unchanged.
Working-tree snapshots retain force-staged ignored files, read physical contents despite
manual index flags, and preserve unmaterialized sparse paths. No Git commit identity is required.
Only the comparison's temporary worktrees are removed on exit; unrelated registrations stay intact.
One effective Cargo build target is supported per run; multi-target arrays fail explicitly.

The verdict is not a proof of compatibility. Behavioral changes, unsupported Rust
patterns, and targets or features outside the selected build still need review.

## GitHub Actions

Use the action in a pull request workflow after checking out the pull request head with complete history:

```yaml
permissions:
  contents: read

steps:
  - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2
    with:
      ref: ${{ github.event.pull_request.head.sha }}
      fetch-depth: 0
      persist-credentials: false
  - uses: ZcashFoundation/zc@v0.7.0
    id: api
    with:
      baseline: origin/${{ github.base_ref }}
      fail-on: breaking
```

The action installs the `cargo-public-api` and Rust nightly versions tested with that `zc` release. It compares the pull request head with its branch point from the baseline, then fails with the same exit-code contract as the CLI. Set `head` when both refs must be compared literally, or enable `with-lock` and `with-values` for the optional analyses. `with-changelog` adds a Keep a Changelog draft for a later step to read.

`fail-on` chooses which findings fail the step: `breaking` (the default), `api-breaking` to let dependency, value, and doc changes pass, `error` for analysis failures only, or `none`. Findings are reported the same way in every mode.

`features` chooses which Cargo features each crate is analysed with: `all` (the default), `default`, or `exclude:<feature>[,<feature>...]`. Use `exclude:` when some features only exist for tests or fuzzing and should not count as public API:

```yaml
  - uses: ZcashFoundation/zc@v0.7.0
    with:
      baseline: origin/${{ github.base_ref }}
      fail-on: api-breaking
      features: exclude:proptest-impl,fuzzing
```

`exclude:` enables every other feature each crate declares at each ref, and also leaves out any feature of that crate that turns on a listed one. A crate that does not declare a listed name ignores it, and zc warns when no workspace crate declares it, which usually means a typo.

The step sets three outputs: `verdict` (`ok`, `breaking`, or `error`), `report`, the path of the JSON report, and `changelog`, the path of the Keep a Changelog draft. A later step can read `${{ steps.api.outputs.verdict }}` or parse the full document. Neither path output ever names a file that does not exist: `report` is empty when the run failed before writing one, and `changelog` is empty unless `with-changelog` is enabled and the analysis produced a draft. Inside Actions, `zc` also annotates the run and writes a summary of the verdict, totals, and per-crate counts to the job summary.

Each action invocation owns its report and changelog files; later invocations do not replace them.

Pin the action to the full commit SHA of a release tag when the workflow requires an immutable dependency. See [SECURITY.md](SECURITY.md).

## Cache

Persistent caches live under the repository root's `target/zc-cache/`, or under
`$CARGO_TARGET_DIR/zc-cache/` when `CARGO_TARGET_DIR` is set. The cache includes
dependency summaries, trait attribution, and per-ref per-crate rustdoc JSON.

Rustdoc JSON is keyed by commit SHA, package name, tool versions, feature policy,
API-affecting environment flags, external Cargo configuration, and cache schema.
Checked-in configuration is covered by the ref SHA. Working-tree JSON is reused
only within the current run, never published to the persistent API cache.
zc removes `*.api.json` files older than 14 days at startup. Delete
`target/zc-cache/` with `rm -rf target/zc-cache` to clear all zc cache
state for a checkout.

## Install

First install the prerequisites (see [Requirements](#requirements)):

```sh
cargo install cargo-public-api --version 0.52.0 --locked
rustup toolchain install nightly-2026-10-15 --profile minimal
```

Then install `zc` itself, a single Rust binary:

```sh
cargo install --git https://github.com/ZcashFoundation/zc --tag v0.7.0 --locked
```

…or from a checkout:

```sh
cargo install --path . --locked
```

## Usage

```sh
zc                       # dirty tree: HEAD -> working tree; clean: branch point with parent -> HEAD
zc main                  # compare against the branch point with main (-> working tree if dirty, else HEAD)
zc v4.1.0 v4.2.0         # compare two arbitrary refs (exact, no merge-base)
zc --with-lock           # include the transitive Cargo.lock diff
zc --with-values main    # also flag const/static value + doc changes
zc --json main           # machine-readable output for CI
zc --report zc.json main # also write the JSON report to a file
zc --fail-on none main   # report every finding without failing the run
zc --features exclude:proptest-impl,fuzzing main # leave test and fuzz features out of the diff
zc --changelog main      # draft a Keep a Changelog changelog (markdown)
zc --changelog-file c.md main # also write that draft to a file
zc --version             # print the installed version
```

Run `zc --help` for the full option and output reference.

### `--json`

`--json` writes a single JSON document to stdout. Progress and diagnostics go to
stderr. `--report <path>` writes that same document to a file in any mode, so a
run can keep the human report on stdout and still hand the JSON to another step.
The report directory must already exist.

Top-level fields:

- `features`: the feature policy the run used, such as `all` or `exclude:fuzzing,proptest-impl`
- `verdict`: `ok`, `breaking`, or `error`
- `totals`: API, dependency, value, doc, public-dep, and error counts
- `deps`, `values`, `docs`: structured diff details
- `public_dep_breaks`: `[{crate, dep, old, new, old_package, new_package, target, class}]` —
  crates whose public API re-exposes a changed external dependency. The `old` and `new`
  fields retain version requirements; `old_package` and `new_package` identify packages.
  Class `breaking` means a package replacement or provably semver-incompatible requirement;
  `review` means the requirement text does not settle compatibility, such as a changed ceiling.
  Only `breaking` entries count toward `totals.public_dep_breaking` and the verdict.
- `crates`: sorted library results with `change` (`existing`, `added`, or `removed`),
  `lib_name`, and `breaking_added` reasons as well as API counts and errors.
- `skipped_crates`: members with no library target at either revision.
- `lock`: `null` unless requested; otherwise `removed`/`added` `{name, version}` rows,
  `changed` `{name, old, new}` rows, and `via` attribution by dependency name.
- `totals.breaking_added`: breaking additions included in `totals.api_breaking`.

A crate error has this shape:

```json
{
  "name": "zebra-state",
  "change": "existing",
  "lib_name": "zebra_state",
  "breaking_added": [],
  "removed": 0,
  "changed": 0,
  "added": 0,
  "status": "error",
  "error": {
    "stage": "head_build",
    "ref": "HEAD",
    "ref_sha": "abc123...",
    "command": "cargo public-api --all-features -p zebra-state -ss",
    "stderr": "... tail of the underlying tool stderr ...",
    "hint": "The crate did not compile under the selected feature set. Fix the build, or choose a feature policy it builds under with --features."
  }
}
```

`stage` is one of `baseline_build`, `head_build`, or `diff`. `command` never
uses cargo-public-api's git ref-diff form. Build errors show the single-ref
command to run in a checkout of `ref_sha`; diff errors show the rustdoc JSON
diff form.

JSON consumers should treat `verdict: error` as an inconclusive analysis. zc
never falls back to fewer features on its own, because doing so can hide
feature-gated public API. A crate whose surface cannot be documented under the
selected `--features` policy must be fixed, or analysed under a policy it builds
with.

### `--changelog`

`--changelog` drafts a [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
changelog (markdown) instead of the diff: one `## <crate>` section per changed
crate, with `### Added` / `### Changed` / `### Removed` lists.

- Added/removed API items are grouped under their owning type, with own-crate
  paths made crate-relative (e.g. `error::TransactionError`) and foreign-type
  paths kept in full. Several members of one type are brace-grouped onto a single
  bullet: `` - `OutPoint::{NULL, new, read, write}` ``. When that brace
  line would exceed ~100 chars, it breaks onto a `` `Type`: `` header with one
  2-space-indented `` - `member` `` per line.
- Changed items retain meaningful attributes and show the old → new signature.
- Trait-impl members use their rustdoc owner and trait identity. Generic arguments
  retain qualified paths, including own-crate prefixes, and explicit binders remain intact.
  Inherent or ambiguous members keep their own item paths.
- Pure machinery is dropped: proptest `Arbitrary` impls, `lazy_static!` wrappers
  (`Deref`/`LazyStatic`/…), and compiler-internal markers (`StructuralPartialEq`).
- Dependency changes are folded into each crate's section: an MSRV
  (`rust-version`) bump and internal workspace bumps
  (`` `zebra-chain` dependency bumped to `10.0.0`. ``) and external migrations
  (`` Migrated to `zcash_primitives 0.27`. ``) under `### Changed`; new and dropped
  dependency uses appear under `### Added` and `### Removed`. Target and build-dependency
  context distinguishes uses of the same package. Dev-only dependencies are omitted from the draft.
- Added crates receive an initial-release entry; removed crates receive a workspace-removal entry.

It's a *draft* — review and curate before committing. Successful drafting obeys
`--fail-on` just like other output modes; a usable draft can accompany exit status 1.

`--changelog-file <path>` writes that same document to a file in any mode, so a
run can keep the human report or the JSON on stdout and still hand the draft to
another step. The changelog directory must already exist, and the destination is
cleared when the run starts. An analysis error leaves no draft: the crates that
failed would be missing from it, and clearing is what keeps that true when an
earlier run already wrote one — otherwise `--fail-on none` would exit 0 over a
stale file.

Keep a Changelog fixes the sections and their order; it does not say which changes
deserve an entry. Curation does: a bug fix or a semantic change that zc's signature
diff cannot see still needs a bullet, an entry describes the change since the crate's
last release rather than since zc's baseline, and it belongs in the commit that makes
the change. The [`skills/zc/`](#claude-code-skill) skill encodes those rules.

### Baseline: the branch point, not the tip

When you compare against a parent branch — the default, or a single explicit
baseline like `zc main` — zc diffs from the **branch point** (the merge-base
of that branch and your head), not the branch's current tip. So commits that
landed on the parent *after* you branched off (or last merged from it) don't
show up as spurious added/removed API, and any changelog built from the diff
describes only what *your* branch changed.

This is computed entirely from local history — no fetch needed, since any parent
commit you merged is by definition already a local ancestor. And because
`merge-base(X, HEAD) == X` whenever `X` is already an ancestor of your head, it's
a no-op for tags and release commits (`zc v4.2.0`) and only moves the baseline
when the parent has genuinely advanced past your branch point. An explicit
two-ref comparison (`zc v4.1.0 v4.2.0`) is always taken literally.

## Claude Code skill

`skills/zc/` bundles a [Claude Code](https://claude.com/claude-code) skill
that drives the full "draft a changelog for a PR" workflow: it runs
`zc --changelog` against the PR's branch point and curates the draft into
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) `CHANGELOG.md` entries
(sections in `Added`/`Changed`/`Deprecated`/`Removed`/`Fixed`/`Security`
order — no `### Breaking Changes` section; breaking changes live under `### Changed`/
`### Removed` as prose, periods only on prose bullets), merged into the existing
`[Unreleased]` section.

Install it for use in any repo by linking it into your personal skills dir:

```sh
mkdir -p ~/.claude/skills
ln -sfn "$PWD/skills/zc" ~/.claude/skills/zc
```

Then ask Claude to "produce the changelog for PR #N" (or invoke `/zc N`).

## Requirements

- [Git 2.34 or newer](https://git-scm.com/docs/git-add/2.34.0)
- [`cargo-public-api`](https://github.com/cargo-public-api/cargo-public-api)
- a `nightly` toolchain for rustdoc JSON builds

## Exit codes

- `0` means clean: no breaking API, dependency, or value changes were found.
- `1` means breaking changes were detected.
- `2` means analysis error: zc could not produce a trustworthy verdict. This
  includes `cargo public-api` or rustdoc failures for one or more crates.
- `64` means usage or setup error, such as an unknown option, bad ref, missing
  required tool, or unsupported shell.

`--fail-on` selects which findings reach `1` and `2`; the codes keep their
meaning. `api-breaking` limits `1` to public API and public-dependency breaks,
`error` reserves failure for an inconclusive analysis, and `none` does not fail
on completed findings. Usage/setup errors and failures before a report is produced
still exit nonzero.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
