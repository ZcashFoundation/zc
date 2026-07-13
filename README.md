# zc

Diff the **public API** and **dependencies** of a Rust workspace's crates
between two git refs — and flag the changes that are breaking for downstream
consumers.

`zc` wraps [`cargo public-api`](https://github.com/cargo-public-api/cargo-public-api)
and adds workspace-wide dependency, lockfile, and const/value diffing on top, so
a single command tells you whether a branch breaks your published surface.

## What it reports

1. **Workspace dependency diff** — each dep classified by the strongest kind it
   is used with (runtime > build > dev); consumer-visible breaking changes
   (runtime removals, major bumps, lost features) are highlighted.
2. **Transitive `Cargo.lock` diff** (`--with-lock`) — transitive version changes,
   each annotated with the direct deps that pull it in.
3. **Per-crate public-API diff** (`cargo public-api`) — removed / changed / added
   items per workspace crate. Built with `--all-features` so feature-gated public
   items are never silently missed. If a crate cannot be analyzed, zc reports
   the failing stage, stderr tail, command, and hint instead of returning an empty
   crate result.
4. **Const/static value & doc-comment diff** (`--with-values`) — catches changes
   `cargo public-api` can't see, since it compares signatures only.
5. **Public-dependency semver breaks** — when a crate re-exposes a foreign type
   (e.g. `-> Result<(), rocksdb::Error>`) whose crate takes a major bump, the
   signature text is unchanged, so `cargo public-api` sees no diff. zc joins the
   crate's external direct-dep major bumps with the foreign roots in its public
   API to flag the break and attribute it to that crate.

It ends with a `BREAKING` / `ERROR` / `OK` verdict. `--json` emits machine-readable output for CI and agents.

`zc` materializes the baseline and head refs in disposable detached worktrees
under a per-run temp directory. Public API builds use target dirs in that temp
directory, so the invoking checkout's HEAD, branch, index, working tree, and
`Cargo.lock` are not checked out over or built in. The worktrees are removed on exit.

## Cache

zc stores persistent cache files under `target/zc-cache/`, or under
`$CARGO_TARGET_DIR/zc-cache/` when `CARGO_TARGET_DIR` is set. The cache includes
resolved dependency summaries, derived value/doc and trait-map indexes, and
per-ref per-crate rustdoc JSON for public API analysis.

Rustdoc JSON cache entries are keyed by commit SHA, crate name,
`cargo-public-api` version, nightly `rustc` version, and the feature policy used
for the run. Working-tree snapshots are not written to the rustdoc JSON cache.
zc removes `*.api.json` files older than 14 days at startup. Delete
`target/zc-cache/` with `rm -rf target/zc-cache` to clear all zc cache
state for a checkout.

## Install

First install the prerequisites (see [Requirements](#requirements)):

```sh
cargo install cargo-public-api    # required; jq is also required
```

Then get `zc` itself — it's a single Bash script. Install it onto your `PATH`:

```sh
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/zc/main/zc -o ~/.local/bin/zc && chmod +x ~/.local/bin/zc
```

…or run a one-off without installing:

```sh
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/zc/main/zc | bash -s -- main
```

In CI, the download-then-run form is the most robust:

```sh
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/zc/main/zc -o zc
chmod +x zc
./zc "$BASE".."$HEAD" --json
```

> When run via `curl … | bash`, `--help` shows only a short usage (the script
> can't re-read its own source from a pipe); install it to a file for the full
> reference.

## Usage

```sh
zc                       # dirty tree: HEAD -> working tree; clean: branch point with parent -> HEAD
zc main                  # compare against the branch point with main (-> working tree if dirty, else HEAD)
zc v4.1.0 v4.2.0         # compare two arbitrary refs (exact, no merge-base)
zc --with-lock           # include the transitive Cargo.lock diff
zc --with-values main    # also flag const/static value + doc changes
zc --json main           # machine-readable output for CI
zc --changelog main      # draft a librustzcash-style changelog (markdown)
zc --version             # version, current commit, and whether it's up to date with origin/main
```

Run `zc --help` for the full option and output reference.

### `--json`

`--json` writes a single JSON document to stdout. Progress and diagnostics go to
stderr.

Top-level fields:

- `verdict`: `ok`, `breaking`, or `error`
- `totals`: API, dependency, value, doc, public-dep, and error counts
- `deps`, `values`, `docs`: structured diff details
- `public_dep_breaks`: `[{crate, dep, old, new}]` — crates whose public API
  re-exposes a major-bumped external dependency
- `crates`: one entry per workspace crate, sorted by crate name

A crate error has this shape:

```json
{
  "name": "zebra-state",
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
    "hint": "The crate did not compile under the selected feature set. Fix the build or choose a supported feature policy."
  }
}
```

`stage` is one of `baseline_build`, `head_build`, or `diff`. `command` never
uses cargo-public-api's git ref-diff form. Build errors show the single-ref
command to run in a checkout of `ref_sha`; diff errors show the rustdoc JSON
diff form.

JSON consumers should treat `verdict: error` as an inconclusive analysis. zc
keeps the `--all-features` policy for the public API diff and does not fall back
to default features automatically, because doing so can hide feature-gated public
API. Crates whose all-features surface cannot be documented must be fixed,
excluded outside zc, or handled by a caller with an explicit feature policy.

### `--changelog`

`--changelog` drafts a [librustzcash](https://github.com/zcash/librustzcash)-style
changelog (markdown) instead of the diff: one `## <crate>` section per changed
crate, with `### Added` / `### Changed` / `### Removed` lists.

- Added/removed API items are grouped under their owning type, with own-crate
  paths made crate-relative (e.g. `error::TransactionError`) and foreign-type
  paths kept in full. Several members of one type are brace-grouped onto a single
  bullet, lrz-style: `` - `OutPoint::{NULL, new, read, write}` ``. When that brace
  line would exceed ~100 chars, it breaks onto a `` `Type`: `` header with one
  2-space-indented `` - `member` `` per line, again matching librustzcash.
- Changed items show the old → new signature, so the entry says *what* changed
  (e.g. a return type going from `Result<Self, E>` to `Self`) rather than just
  naming the item.
- Trait-impl associated items are grouped under an `impl <Trait> for <Self>`
  header. The trait is recovered from the crate's rustdoc JSON (so even a
  *changed* associated type like `ValueBalance::Bytes`, whose `impl` line is
  unchanged and thus absent from the diff, is attributed correctly), and the
  Self generics are recovered from the signature (kept to one module segment,
  lifetimes dropped). A trait implemented on several types collapses to one
  `` `impl Trait` for: `` block; conversely several traits on one type collapse to
  `` `impl {Clone, Debug, ...} for T` ``. The trait's own method (`from`, `clone`,
  …) isn't listed — the `impl` line implies it.
- Pure machinery is dropped: proptest `Arbitrary` impls, `lazy_static!` wrappers
  (`Deref`/`LazyStatic`/…), and compiler-internal markers (`StructuralPartialEq`).
- Dependency changes are folded into each crate's section: an MSRV
  (`rust-version`) bump and internal workspace bumps
  (`` `zebra-chain` dependency bumped to `10.0.0`. ``) and external migrations
  (`` Migrated to `zcash_primitives 0.27`. ``) under `### Changed`, dropped deps
  under `### Removed`.

It's a *draft* — review and curate before committing. Needs a `nightly`
toolchain for the trait attribution (without one it falls back to plain
type grouping).

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

`skills/changelog/` bundles a [Claude Code](https://claude.com/claude-code) skill
that drives the full "draft a changelog for a PR" workflow: it runs
`zc --changelog` against the PR's branch point and curates the draft into
[librustzcash](https://github.com/zcash/librustzcash)-style `CHANGELOG.md` entries
(Keep a Changelog sections in `Added`/`Changed`/`Deprecated`/`Removed`/`Fixed`/`Security`
order — no `### Breaking Changes` section; breaking changes live under `### Changed`/
`### Removed` as prose, periods only on prose bullets), merged into the existing
`[Unreleased]` section.

Install it for use in any repo by linking it into your personal skills dir:

```sh
mkdir -p ~/.claude/skills
ln -sfn "$PWD/skills/changelog" ~/.claude/skills/changelog
```

Then ask Claude to "produce the changelog for PR #N" (or invoke `/changelog N`).

## Requirements

- Bash 4+ (associative arrays). On macOS, zc tries to re-exec with `bash` from PATH, `/opt/homebrew/bin/bash`, or `/usr/local/bin/bash` before failing. Install it with `brew install bash`.
- [`cargo-public-api`](https://github.com/cargo-public-api/cargo-public-api)
- `jq`
- a `nightly` toolchain for rustdoc JSON builds

## Exit codes

- `0` means clean: no breaking API, dependency, or value changes were found.
- `1` means breaking changes were detected.
- `2` means analysis error: zc could not produce a trustworthy verdict. This
  includes `cargo public-api` or rustdoc failures for one or more crates.
- `64` means usage or setup error, such as an unknown option, bad ref, missing
  required tool, or unsupported shell.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
