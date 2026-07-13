---
name: changelog
description: >-
  Produce librustzcash-style CHANGELOG entries for the current branch (or a
  given PR/branch) by running zc and writing them into the repo's CHANGELOG.md
  files; or, with --check, verify the existing entries against zc and report
  discrepancies without writing. Use when the user asks to write/draft/produce or
  check/verify a changelog for a branch or PR, or turn a zc diff into entries.
---

# Produce librustzcash-style changelogs with zc

Run `zc --changelog`, curate its draft into
[librustzcash](https://github.com/zcash/librustzcash)-style entries, and
**write them into the repo's `CHANGELOG.md` files**. Needs `zc` on `PATH` (plus
its prereqs: `cargo-public-api`, `jq`, a nightly toolchain).

## Quick start

For the current branch, with a clean working tree:

```
zc --changelog
```

zc prints per-crate `## <crate>` sections holding `### Added/Changed/Removed`
bullets, paths already crate-relative and type members brace-grouped. Treat that
output as a **draft**: place breaking changes in their natural section (step 3),
sort into sections (step 4), then write the result into the matching `CHANGELOG.md`
files.

To verify existing entries instead of writing them, use `--check` (last section): it
reports discrepancies against zc and writes nothing, the fast pre-PR gate.

## Workflow

### 1. Resolve the diff range
- No argument means the current branch: zc diffs the committed branch against its
  branch point with `main`. Commit work-in-progress first so the tree is clean,
  otherwise zc diffs only the uncommitted edits.
- A PR number or ref means that target instead: `gh pr view <N> --json
  headRefName,headRefOid,baseRefName`. If it is checked out locally, treat it like
  the current branch; otherwise `git fetch <remote> pull/<N>/head` and pass explicit
  refs in step 2.

### 2. Run zc
- Current branch: `zc --changelog`.
- Explicit refs: `zc --changelog $(git merge-base <base> <head>) <head>`.
- zc runs a full `cargo public-api` build with `--all-features`. Run it where
  builds are fast, such as a remote build host for large workspaces.
- Exit `1` means zc found breaking changes and can still draft a changelog.
  Exit `2` means analysis failed for at least one crate; do not curate the
  changelog until the reported stage, stderr, and hint are resolved.
- An empty draft is a valid result: no public API changed, so there are no
  `### Added`/`### Changed`/`### Removed` entries to write. A `### Fixed`,
  `### Deprecated`, or `### Security` entry may still come from the PR (step 4).

### 3. Place breaking changes in their natural section
librustzcash has **no `### Breaking Changes` section** — Keep a Changelog does not
define one, and lrz does not add it. A breaking change lives in the section that
names what happened; the crate's semver **major** bump (chosen by the version-bump
step, not here) is what records that the release breaks:
- **Removed**: removing any public item. Under `### Removed`.
- **Changed**: a changed signature, parameter list, field, or type. Under
  `### Changed`, as prose stating the new behavior and what callers must do —
  "`Foo::bar` now takes a `NonZeroU8` instead of a `u8`." (see REFERENCE.md).
- **Added**: additions stay under `### Added`, even the ones that force a major bump.
  These look additive but break downstream — leave them under `### Added` (lrz lists
  new variants there, e.g. `TxVersion::V6`) and let the version bump carry the break:
  - a new variant on an enum that is **not `#[non_exhaustive]`** (exhaustive `match`es
    stop compiling);
  - a variant changing kind, e.g. unit to struct or tuple, on such an enum;
  - a new public field on a struct callers build with a struct literal, when every
    field is public and the struct is **not `#[non_exhaustive]`**;
  - a new method on a public trait downstream code implements (a trait only this
    crate implements, e.g. a generated server trait, is not breaking).

Reserve an inline **BREAKING CHANGES** marker (bold, at the start of the bullet) for
an exceptionally disruptive change such as a wholesale database-schema migration, as
lrz uses it rarely. Do not tag every breaking bullet, and never add a `(breaking; …)`
gloss.

### 4. Sort into sections
- **Order** (Keep a Changelog): `### Added`, `### Changed`, `### Deprecated`,
  `### Removed`, `### Fixed`, `### Security`. Include only sections with entries.
- **Source**: zc fills `Added`/`Changed`/`Removed` from the API and dependency
  diff. `Fixed`, `Deprecated`, and `Security` have no API signal, so take them from
  the PR.
- **One section per change**: when a change fits several, use the most impactful.
  Priority: Security > Removed > Changed > Deprecated > Added > Fixed.
- **Tone**: plain and factual. No hyperbole ("comprehensive", "significant"),
  marketing ("game-changing"), intensifiers ("greatly improved"), or hedging
  ("helps to", "aims to").

For wording, periods, width, and brace-group layout, see [REFERENCE.md](REFERENCE.md).
For a full draft-to-entries walkthrough, see [EXAMPLES.md](EXAMPLES.md).

### 5. Write to the CHANGELOG files
- For each `## <crate>` zc reports, insert the curated entries into that crate's
  `CHANGELOG.md` `[Unreleased]` section, merging under the right subsections. Do not
  clobber existing entries, and skip anything already documented.
- Add a plain-language line to the workspace `CHANGELOG.md` for user-facing
  features; skip experimental or feature-gated work.
- **Leave the edits unstaged** so they show up in `git diff` for review.
- Show a `git diff --stat` of the touched files and audit: no bare-identifier bullet
  has a trailing period, prose bullets do, every line is 100 chars or fewer.

## `--check` mode: verify, do not write

`--check` (optionally with a PR or branch arg) runs the diff (steps 1 and 2) but
**reports discrepancies and writes nothing**: the fast pre-PR gate for the "looks
public but isn't" error a human cannot eyeball.

1. From zc's output, collect the public-API items: the backtick'd identifiers
   under each crate's `### Added` / `### Removed`, plus the old and new of
   `### Changed`.
2. Collect only the entries **this branch adds**, not the whole `[Unreleased]`
   section (which also holds other PRs' entries that zc, diffing only this branch,
   will not report). Take the added (`+`) bullets from `git diff <branch-point>..HEAD
   -- '**/CHANGELOG.md' 'CHANGELOG.md'`, where branch-point is `git merge-base main
   HEAD` (the same baseline zc used), keeping only the bare backtick'd identifier
   bullets. Ignore prose and behavioral bullets, which zc cannot see.
3. Report two lists and edit nothing:
   - **Listed but not public**: branch-added identifier bullets zc does not
     report. Usually a non-public path (for example behind a `pub(crate)` module), or
     a line that belongs in prose rather than the identifier list.
   - **Public but undocumented**: items zc reports that the branch did not add.

   Both lists empty means the changelog's API entries match the public surface.

## Notes
- Library-crate changelogs follow librustzcash style (terse, code-pathed); the
  workspace `CHANGELOG.md` uses plain, user-facing descriptions.
- zc's baseline is the branch point by default, so a stale local `main` or a
  branch that is behind upstream will not pollute the diff.
