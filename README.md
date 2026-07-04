# ziff

Diff the **public API** and **dependencies** of a Rust workspace's crates
between two git refs — and flag the changes that are breaking for downstream
consumers.

`ziff` wraps [`cargo public-api`](https://github.com/cargo-public-api/cargo-public-api)
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
   items are never silently missed. If a crate cannot be analyzed, ziff reports
   the failing stage, stderr tail, command, and hint instead of returning an empty
   crate result.
4. **Const/static value & doc-comment diff** (`--with-values`) — catches changes
   `cargo public-api` can't see, since it compares signatures only.

It ends with a `BREAKING` / `ERROR` / `OK` verdict. `--json` emits machine-readable output for CI and agents.

`ziff` materializes the baseline and head refs in disposable detached worktrees
under a per-run temp directory. Public API builds use target dirs in that temp
directory, so the invoking checkout's HEAD, branch, index, working tree, and
`Cargo.lock` are not checked out over or built in. The worktrees are removed on exit.

## Install

First install the prerequisites (see [Requirements](#requirements)):

```sh
cargo install cargo-public-api    # required; jq is also required
```

Then get `ziff` itself — it's a single Bash script. Install it onto your `PATH`:

```sh
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/ziff/main/ziff -o ~/.local/bin/ziff && chmod +x ~/.local/bin/ziff
```

…or run a one-off without installing:

```sh
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/ziff/main/ziff | bash -s -- main
```

In CI, the download-then-run form is the most robust:

```sh
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/ziff/main/ziff -o ziff
chmod +x ziff
./ziff "$BASE".."$HEAD" --json
```

> When run via `curl … | bash`, `--help` shows only a short usage (the script
> can't re-read its own source from a pipe); install it to a file for the full
> reference.

## Usage

```sh
ziff                       # dirty tree: HEAD -> working tree; clean: branch point with parent -> HEAD
ziff main                  # compare against the branch point with main (-> working tree if dirty, else HEAD)
ziff v4.1.0 v4.2.0         # compare two arbitrary refs (exact, no merge-base)
ziff --with-lock           # include the transitive Cargo.lock diff
ziff --with-values main    # also flag const/static value + doc changes
ziff --json main           # machine-readable output for CI
ziff --changelog main      # draft a librustzcash-style changelog (markdown)
```

Run `ziff --help` for the full option and output reference.

### `--json`

`--json` writes a single JSON document to stdout. Progress and diagnostics go to
stderr.

Top-level fields:

- `verdict`: `ok`, `breaking`, or `error`
- `totals`: API, dependency, value, doc, and error counts
- `deps`, `values`, `docs`: structured diff details
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

JSON consumers should treat `verdict: error` as an inconclusive analysis. ziff
keeps the `--all-features` policy for the public API diff and does not fall back
to default features automatically, because doing so can hide feature-gated public
API. Crates whose all-features surface cannot be documented must be fixed,
excluded outside ziff, or handled by a caller with an explicit feature policy.

### `--changelog`

`--changelog` drafts a [librustzcash](https://github.com/zcash/librustzcash)/Zebra-style
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
baseline like `ziff main` — ziff diffs from the **branch point** (the merge-base
of that branch and your head), not the branch's current tip. So commits that
landed on the parent *after* you branched off (or last merged from it) don't
show up as spurious added/removed API, and any changelog built from the diff
describes only what *your* branch changed.

This is computed entirely from local history — no fetch needed, since any parent
commit you merged is by definition already a local ancestor. And because
`merge-base(X, HEAD) == X` whenever `X` is already an ancestor of your head, it's
a no-op for tags and release commits (`ziff v4.2.0`) and only moves the baseline
when the parent has genuinely advanced past your branch point. An explicit
two-ref comparison (`ziff v4.1.0 v4.2.0`) is always taken literally.

## Claude Code skill

`skills/changelog/` bundles a [Claude Code](https://claude.com/claude-code) skill
that drives the full "draft a changelog for a PR" workflow: it runs
`ziff --changelog` against the PR's branch point and curates the draft into
librustzcash/Zebra-style `CHANGELOG.md` entries (prose `### Changed`, periods only
on prose bullets, `### Breaking Changes` for new variants on non-`#[non_exhaustive]`
enums, merged into the existing `[Unreleased]` section).

Install it for use in any repo by linking it into your personal skills dir:

```sh
mkdir -p ~/.claude/skills
ln -sfn "$PWD/skills/changelog" ~/.claude/skills/changelog
```

Then ask Claude to "produce the changelog for PR #N" (or invoke `/changelog N`).

## Requirements

- Bash 4+ (associative arrays). On macOS, ziff tries to re-exec with `bash` from PATH, `/opt/homebrew/bin/bash`, or `/usr/local/bin/bash` before failing. Install it with `brew install bash`.
- [`cargo-public-api`](https://github.com/cargo-public-api/cargo-public-api)
- `jq`
- a `nightly` toolchain for rustdoc JSON builds

## Exit codes

- `0` means clean: no breaking API, dependency, or value changes were found.
- `1` means breaking changes were detected.
- `2` means analysis error: ziff could not produce a trustworthy verdict. This
  includes `cargo public-api` or rustdoc failures for one or more crates.
- `64` means usage or setup error, such as an unknown option, bad ref, missing
  required tool, or unsupported shell.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
