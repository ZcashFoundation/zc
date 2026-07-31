---
name: zc
description: >-
  Produce librustzcash-style CHANGELOG entries for the current branch (or a
  given PR/branch) by running zc and writing them into the repo's CHANGELOG.md
  files; or, with `/zc --check`, verify the existing entries against zc and report
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
sort into sections (step 4), reconcile against the last release (step 5), then write
the result into the matching `CHANGELOG.md` files.

To verify existing entries instead of writing them, use `--check` (last section): it
reports discrepancies against zc and writes nothing, the fast pre-PR gate.

## What requires an entry

The zc diff is the API signal, not the whole requirement. [librustzcash
CONTRIBUTING][lrz-changelog] — the style zc targets — requires an entry for:

- any change to a crate's public API;
- any bug fix;
- any change to the semantics of existing behavior, **even with the signature
  untouched**: stricter validation, different equality or ordering, a previously fixed
  value becoming configurable. The tool cannot see these; take them from the PR.
- privacy, security, and cost properties of a public API, which are user-facing even
  when documented only in code comments;
- a version bump of a dependency **whose types appear in the public API**. Types from
  two semver-incompatible versions of a crate do not unify, so a consumer has to
  upgrade that dependency in lockstep — that is the rationale lrz gives for recording
  these ([zebra#11111][dep-rationale]). Rust-flavored semver makes `0.29 → 0.30`
  breaking here. The draft covers this: zc emits one `Migrated to …` bullet per
  external requirement change, and when the dep is major-bumped and reachable in the
  crate's public API — the case where no signature text changed, so `cargo public-api`
  sees nothing — that same bullet carries the lockstep-upgrade note.

The exception is a crate that has **never been released**: its changelog gets only
`- Initial release!` under the version heading, with no itemized API. There is no prior
release for a user to adapt from, so discard zc's draft for such a crate.

[lrz-changelog]: https://github.com/zcash/librustzcash/blob/main/CONTRIBUTING.md#changelog-entries
[zebra-changelog]: https://github.com/ZcashFoundation/zebra/blob/main/book/src/dev/changelog-guidelines.md
[dep-rationale]: https://github.com/ZcashFoundation/zebra/pull/11111#issuecomment-5097643724

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
- **Audience**: an entry carries only what a consumer of the public API needs in order
  to adapt. `Added` bullets are pointers — name the item and let its `rustdoc` explain
  it. `Changed` says what a caller must do differently. Implementation details, internal
  refactors, test-fixture reworks, and contracts not observable through the public API
  do not belong in a changelog at all; drop them from the draft.
- **Tone**: plain and factual. No hyperbole ("comprehensive", "significant"),
  marketing ("game-changing"), intensifiers ("greatly improved"), or hedging
  ("helps to", "aims to").

For wording, periods, width, and brace-group layout, see [REFERENCE.md](REFERENCE.md).
For a full draft-to-entries walkthrough, see [EXAMPLES.md](EXAMPLES.md).

### 5. Reconcile with the existing `[Unreleased]` section
An entry describes the difference between the **last released version of the crate** and
the state your change produces — not the difference from the tool's baseline. Diffing
starts at the branch point, so on a stacked or long-lived branch the draft can name
interstitial states no user ever saw. Collapse them:

- An unreleased item this branch renames yields **one** entry, naming the final name.
  Edit the existing `[Unreleased]` bullet in place; do not add a removal plus an addition.
- An unreleased item this branch removes leaves **no** entry: delete the `[Unreleased]`
  bullet that added it.
- Anything else already under `[Unreleased]` — update that entry to the new net state
  rather than appending a second one describing the delta from the first.

The reference point is the last release **on the branch you target**: a fix branched from
a `maint/` branch describes its change relative to that branch's most recent release,
which may be older than the most recent release on `main`.

### 6. Write to the CHANGELOG files
- For each `## <crate>` zc reports, insert the curated entries into that crate's
  `CHANGELOG.md` `[Unreleased]` section, merging under the right subsections. Do not
  clobber existing entries, and skip anything already documented.
- A repo with both a library-crate changelog and an operator-facing binary changelog
  (Zebra: `<crate>/CHANGELOG.md` vs the root `CHANGELOG.md`) routes by audience, and the
  dependency rule above applies only to the **crate** changelogs. Their readers are
  downstream Rust code, for whom a semver-incompatible bump is a break they must match
  in lockstep. Node operators do not consume Rust APIs, so
  [Zebra's guidelines][zebra-changelog] exclude dependency-only bumps from the root
  changelog unless they carry an operator-relevant security fix.
  Add a plain-language line to the root changelog for user-facing features only; skip
  experimental or feature-gated work.
- **Leave the edits unstaged** so they show up in `git diff` for review.
- Show a `git diff --stat` of the touched files and audit: no bare-identifier bullet
  has a trailing period, prose bullets do, every line is 100 chars or fewer.
- The entry belongs in the **same commit as the change it describes**, never a trailing
  "update changelogs" commit: a public API change is not complete until its
  documentation exists, and keeping the two together means the entry travels with the
  commit when it is cherry-picked or forward-merged. Once the diff has been reviewed,
  fold each entry into its commit with [`git revise`](https://github.com/mystor/git-revise).
- A released `## [x.y.z] - DATE` section is the historical record of what that release
  shipped. Correct a bullet there when it was wrong as written — an inaccurate record is
  worse than an edited one — but never record anything that happened afterwards, such as
  a later re-export or a clarification prompted by a subsequent change. That belongs
  under `[Unreleased]`, where the users who need it will look.
- The `## [Unreleased]` heading is permanent. Keep it even when the section is empty.

## Release changelogs (the release-plz PR)

A release differs from a normal PR in two ways that break the default workflow, and both
cause per-crate `` `x` dependency bumped to `y`. `` entries to be silently omitted:

1. **The version bumps live in the release PR branch, not `main`.** release-plz writes the
   per-crate `Cargo.toml`/`Cargo.lock` bumps into its `release-plz-*` branch. A default
   `zc --changelog` diffs against `main`, sees **no** dependency-version changes, and so drops
   every dependency-bump entry — leaving dependency-propagation crates undocumented.
2. **The baseline is the previous release, not the branch point.** Diff from the last published
   release, not `merge-base(main, HEAD)`.

So for a release, run zc against the **release PR branch**, based on the previous release:

```
zc --changelog <previous-release-ref> <release-plz-branch-HEAD>
```

- Run it **after** any manual version overrides land on the branch. If you correct release-plz's
  computed versions, zc must see the final numbers or it documents the wrong bumps.
- zc's per-crate `### Changed` dependency-bump list is the **source of truth** — include all of
  it. A run against `main` cannot produce it.
- Write the entries into the **release PR branch's** `CHANGELOG.md` files (they merge to `main`
  with the release). Do **not** open a separate `main` PR for them: a new `main` push makes
  release-plz regenerate the release PR and discard any manual version overrides on it.
- Skip dev-dependency-only bumps (stripped from published crates, so not consumer-visible), and
  skip the dep list for a binary crate like `zebrad` (its changelog is operator-facing prose).

## `--check` mode: verify, do not write

A mode of **this skill**, not a flag on the `zc` binary: invoke it as `/zc --check`
(optionally with a PR or branch arg). Passing `--check` to `zc` itself exits 64,
unknown option. The skill runs the ordinary diff (steps 1 and 2), then compares —
**reporting discrepancies and writing nothing**: the fast pre-PR gate for the
"looks public but isn't" error a human cannot eyeball.

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

   Both lists cover only the API surface. Two requirements have no zc signal, so check
   them by hand against the PR: a bug fix or a semantics change with an untouched
   signature needs an entry too, and a commit that changes the public API must carry its
   entry in that same commit.

## Notes
- Library-crate changelogs follow librustzcash style (terse, code-pathed); the
  workspace `CHANGELOG.md` uses plain, user-facing descriptions.
- zc's baseline is the branch point by default, so a stale local `main` or a
  branch that is behind upstream will not pollute the diff.
