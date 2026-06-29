---
name: changelog
description: >-
  Produce librustzcash/Zebra-style CHANGELOG entries for the current branch (or a
  given PR/branch) by running ziff and writing them into the repo's CHANGELOG.md
  files; or, with --check, verify the existing entries against ziff and report
  discrepancies without writing. Use when the user asks to write/draft/produce or
  check/verify a changelog for a branch or PR, or turn a ziff diff into entries.
---

# Produce librustzcash/Zebra-style changelogs with ziff

Run `ziff --changelog`, curate its draft into
[librustzcash](https://github.com/zcash/librustzcash)/Zebra-style entries, and
**write them into the repo's `CHANGELOG.md` files**. Needs `ziff` on `PATH` (plus
its prereqs: `cargo-public-api`, `jq`, a nightly toolchain).

## Quick start

For the current branch, with a clean working tree:

```
ziff --changelog
```

ziff prints per-crate `## <crate>` sections holding `### Added/Changed/Removed`
bullets, paths already crate-relative and type members brace-grouped. Treat that
output as a **draft**: classify breaking changes (step 3), sort the rest into
sections (step 4), then write the result into the matching `CHANGELOG.md` files.

To verify existing entries instead of writing them, use `--check` (last section): it
reports discrepancies against ziff and writes nothing, the fast pre-PR gate.

## Workflow

### 1. Resolve the diff range
- No argument means the current branch: ziff diffs the committed branch against its
  branch point with `main`. Commit work-in-progress first so the tree is clean,
  otherwise ziff diffs only the uncommitted edits.
- A PR number or ref means that target instead: `gh pr view <N> --json
  headRefName,headRefOid,baseRefName`. If it is checked out locally, treat it like
  the current branch; otherwise `git fetch <remote> pull/<N>/head` and pass explicit
  refs in step 2.

### 2. Run ziff
- Current branch: `ziff --changelog`.
- Explicit refs: `ziff --changelog $(git merge-base <base> <head>) <head>`.
- ziff runs a full `cargo public-api` build, so run it where builds are fast. A
  flaky build can exit non-zero; rerun to confirm before treating it as a real
  failure.
- An empty draft is a valid result: no public API changed, so there are no
  `### Added`/`### Changed`/`### Removed` entries to write. A `### Fixed`,
  `### Deprecated`, or `### Security` entry may still come from the PR (step 4).

### 3. Classify breaking changes
Put every breaking item under `### Breaking Changes` with a trailing
`(breaking; needs a major version bump)` gloss. By the priority in step 4, Breaking
Changes outranks Removed, Changed, and Added, so a breaking item lives there, not in
the section ziff drafted it under. Breaking items come from all three ziff sections:
- **Removed**: removing any public item is breaking.
- **Changed**: a changed signature, parameter list, field, or type is breaking.
- **Added**: most additions are safe, but each of the following looks additive yet
  breaks downstream, so check it before leaving it under `### Added`:
  - a new variant on an enum that is **not `#[non_exhaustive]`** (exhaustive `match`es
    stop compiling);
  - a variant changing kind, e.g. unit to struct or tuple, on such an enum (same break);
  - a new public field on a struct callers build with a struct literal, when every
    field is public and the struct is **not `#[non_exhaustive]`** (the literal stops
    compiling; a derived `Default` does not rescue this if any caller names every
    field instead of using `..Default::default()`);
  - a new method on a public trait that downstream code implements (their impls stop
    compiling); a trait only this crate implements (e.g. a generated server trait) is
    not breaking.

Everything else ziff lists is genuinely additive.

### 4. Sort into sections
- **Order**: `### Breaking Changes`, `### Added`, `### Changed`, `### Deprecated`,
  `### Removed`, `### Fixed`, `### Security`. Include only sections with entries.
- **Source**: ziff fills `Added`/`Changed`/`Removed` from the API and dependency
  diff. `Fixed`, `Deprecated`, and `Security` have no API signal, so take them from
  the PR.
- **One section per change**: when a change fits several, use the most impactful.
  Priority: Breaking Changes > Security > Removed > Changed > Deprecated > Added >
  Fixed.
- **Tone**: plain and factual. No hyperbole ("comprehensive", "significant"),
  marketing ("game-changing"), intensifiers ("greatly improved"), or hedging
  ("helps to", "aims to").

For wording, periods, width, and brace-group layout, see [REFERENCE.md](REFERENCE.md).
For a full draft-to-entries walkthrough, see [EXAMPLES.md](EXAMPLES.md).

### 5. Write to the CHANGELOG files
- For each `## <crate>` ziff reports, insert the curated entries into that crate's
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

1. From ziff's output, collect the public-API items: the backtick'd identifiers
   under each crate's `### Added` / `### Removed`, plus the old and new of
   `### Changed`.
2. Collect only the entries **this branch adds**, not the whole `[Unreleased]`
   section (which also holds other PRs' entries that ziff, diffing only this branch,
   will not report). Take the added (`+`) bullets from `git diff <branch-point>..HEAD
   -- '**/CHANGELOG.md' 'CHANGELOG.md'`, where branch-point is `git merge-base main
   HEAD` (the same baseline ziff used), keeping only the bare backtick'd identifier
   bullets. Ignore prose and behavioral bullets, which ziff cannot see.
3. Report two lists and edit nothing:
   - **Listed but not public**: branch-added identifier bullets ziff does not
     report. Usually a non-public path (for example behind a `pub(crate)` module), or
     a line that belongs in prose rather than the identifier list.
   - **Public but undocumented**: items ziff reports that the branch did not add.

   Both lists empty means the changelog's API entries match the public surface.

## Notes
- Library-crate changelogs follow librustzcash style (terse, code-pathed); the
  workspace `CHANGELOG.md` uses plain, user-facing descriptions.
- ziff's baseline is the branch point by default, so a stale local `main` or a
  branch that is behind upstream will not pollute the diff.
