---
name: zc
description: >-
  Produce or verify changelog entries for a branch or PR using zc's API and
  dependency analysis. Detect the repo's source of truth first: write Changie
  fragments for Zebra, or direct CHANGELOG.md entries in non-Changie repos.
  `/zc --check` reports discrepancies without writing. Use when asked to
  write/draft/produce or check/verify a changelog, or turn a zc diff into entries.
---

# Produce repository-native changelogs with zc

Be laconic in chat and public prose: answer directly, without preambles,
repetition, or unsolicited explanations. Expand only when asked or necessary
for correctness; preserve required sections and evidence.

Run `zc --changelog`, curate its Markdown draft, and write to the repo's **changelog
source**, not necessarily `CHANGELOG.md`. zc remains the API/dependency analyzer:
`--json` or `--report <path>` provides machine-readable evidence; no YAML-output
flag is needed. Requires `zc` on `PATH`, `cargo-public-api`, and a nightly toolchain.

## Quick start

For the current branch, with a clean working tree:

```
zc --changelog
```

zc prints per-crate `## <crate>` sections holding `### Added/Changed/Removed`
bullets, paths already crate-relative and type members brace-grouped. Treat the
output as a **draft**: repository policy decides categories and where to write.
For Zebra, use `changie new -j PROJECT -k KIND -b BODY` to create fragments; never
paste the draft into generated changelogs. See [EXAMPLES.md](EXAMPLES.md).

To verify instead, invoke `/zc --check` (last section). This is a skill mode,
not a `zc` binary flag, and writes nothing.

## What requires an entry

The zc diff is the API signal, not the whole requirement. Repository policy decides
what belongs in a changelog; ordinarily an entry is required for:

- any change to a crate's public API;
- any bug fix;
- any change to the semantics of existing behavior, **even with the signature
  untouched**: stricter validation, different equality or ordering, a previously fixed
  value becoming configurable. The tool cannot see these; take them from the PR.
- privacy, security, and cost properties of a public API, which are user-facing even
  when documented only in code comments;
- a version bump of a dependency **whose types appear in the public API**. Types from
  two semver-incompatible versions of a crate do not unify, so a consumer has to
  upgrade that dependency in lockstep, which is why these are recorded
  ([zebra#11111][dep-rationale]). Rust-flavored semver makes `0.29 → 0.30`
  breaking here. The draft covers this: zc emits one `Migrated to …` bullet per
  external requirement change, and when the dep is major-bumped and reachable in the
  crate's public API — the case where no signature text changed, so `cargo public-api`
  sees nothing — that same bullet carries the lockstep-upgrade note.

For a crate that has **never been released**, follow its repository's initial-release
convention rather than itemizing changes from an imaginary prior release. In direct
Markdown repos using the convention below, keep only `- Initial release!`.

[zebra-changelog]: https://github.com/ZcashFoundation/zebra/blob/main/book/src/dev/changelog-guidelines.md
[dep-rationale]: https://github.com/ZcashFoundation/zebra/pull/11111#issuecomment-5097643724

## Workflow
### 0. Find the source of truth
- Read repository changelog policy and generator configuration before choosing a writer.
  For Zebra, read `.changie.yaml` and `book/src/dev/changelog-guidelines.md`; for a
  release, also read `release-plz.toml`, `.github/workflows/release.yml`, and
  `.github/scripts/batch-release-changelogs.sh`.
- **Zebra / Changie:** `.changes/unreleased/*.yaml` are ordinary PR inputs;
  `.changes/<project>/v<version>.md` are batched release and historical sources.
  `.changie.yaml` maps project `zebrad` to root `CHANGELOG.md` and library project keys
  to their crate changelogs. Those `CHANGELOG.md` files are generated: never edit them
  by hand. Use the configured keys, not a guessed crate directory or display label.
- **Non-Changie:** use the repo's established direct Markdown workflow below, unless
  another generator owns the files. Repository policy wins over generic Keep a Changelog.


### 1. Resolve the diff range
- No argument means the current branch. On a clean tree, zc compares its branch point
  with the parent (upstream tracking ref, falling back to `main`) to `HEAD`. On a dirty
  tree, the default instead compares `HEAD` to working-tree edits. Do not silently use
  that narrower range for a whole-PR changelog; pass explicit committed refs.
- Resolve a PR with `gh pr view <N> --json headRefName,headRefOid,baseRefName`;
  fetch its head if needed. For an ordinary PR, use its actual target branch to find
  the merge-base, including a `maint/` target rather than assuming `main`.
- A release uses each crate's previous published release to the exact final release-PR
  head, not a branch point; see the release section.

### 2. Run zc
- Current branch: `zc --changelog`.
- Explicit refs: `zc --changelog $(git merge-base <base> <head>) <head>`.
- zc runs a full `cargo public-api` build with `--all-features`. Run it where
  builds are fast, such as a remote build host for large workspaces.
- With the default failure policy, exit `1` means breaking changes were found and the
  draft is usable. Exit `2` means analysis failed; resolve the reported stage, stderr,
  and hint before curating. Exit `64` means usage or setup failed.
- An empty draft means no API/dependency entries were emitted, not that the PR needs
  no entry. Behavioral, bug-fix, deprecation, and security changes can come from the PR.

### 3. Classify breaking changes by repository policy
**Zebra:** use CLI kind `breaking`, rendered as `### Breaking Changes`. A breaking
library addition needs **both** a `breaking` fragment explaining its impact and an
`Added` fragment naming the capability. Examples include:
- a new variant on an enum without `#[non_exhaustive]`;
- a new public field on a fully public struct constructed with literals, without
  `#[non_exhaustive]`;
- a required method on a public trait downstream code implements.

Other breaks, such as removing an API or changing a variant's shape/signature, belong
under `breaking`, not automatically under both kinds. Judge `zebrad` breaks by operator
impact (configuration, CLI, RPC, database compatibility), not by Rust API changes.

**Direct Markdown / generic Keep a Changelog:** absent a repository-specific breaking
section, put removals in `Removed`, signature/behavior/type changes in `Changed`, and
additions in `Added`, even when additive-looking changes break downstream code.
The release process, not this skill, decides the version bump. Reserve a rare inline
**BREAKING CHANGES** marker for an exceptionally disruptive change if that repo uses it;
do not add a `(breaking; …)` gloss to every entry.

### 4. Sort and curate
- **Zebra order:** `Breaking Changes`, `Added`, `Changed`, `Deprecated`, `Removed`,
  `Fixed`, `Security`. CLI kinds are `breaking`, `Added`, `Changed`, `Deprecated`,
  `Removed`, `Fixed`, `Security`. Follow `.changie.yaml` rather than guessing YAML
  `kind` serialization from the CLI key or rendered heading.
- **Generic order:** `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.
  Include only sections with entries.
- **Source:** zc supplies API/dependency evidence. Behavioral changes and fixes also
  need PR review; an unchanged signature does not mean unchanged behavior.
- **One distinct change per entry:** choose the most impactful category. Zebra's
  priority is breaking > Security > Removed > Changed > Deprecated > Added > Fixed,
  except for its required breaking-plus-Added pair. Generic policy omits breaking
  from that priority. Do not combine independent changes or PRs into one entry.
- **Zebra Fixed vs Changed:** use `Fixed` only when the fix is invisible to users.
  Changed output, error messages, logs, or behavior belongs in `Changed` (or a higher
  priority category).
- **Audience:** library entries tell downstream callers what is new or what they must
  do differently. `zebrad` entries describe operator-visible behavior in plain language.
  Dependency-only bumps belong in library entries, not `zebrad`, unless they carry an
  operator-relevant security fix. Drop internal refactors and test-only changes.
- **Tone:** plain and factual. No hyperbole ("comprehensive", "significant"),
  marketing ("game-changing"), intensifiers ("greatly improved"), or hedging
  ("helps to", "aims to").

For wording, periods, width, and brace-group layout, see [REFERENCE.md](REFERENCE.md).
For a full draft-to-entries walkthrough, see [EXAMPLES.md](EXAMPLES.md).

### 5. Reconcile existing source entries
An entry describes the difference between the **last released version of the crate**
and the final change, not intermediate states no user saw. Read the relevant project's
existing fragments, or its `[Unreleased]` entries in a direct Markdown repo:

- If this branch renames an unreleased item, update the original source entry to its
  final name; do not append a removal plus an addition.
- If it removes an unreleased addition, remove that source entry; no entry remains.
- If it changes something already documented, update the source to the net result,
  avoiding duplicates and retaining unrelated changes and PR links.
- On a release branch, fragments may already be consumed. Reconcile the target
  `.changes/<project>/v<version>.md`, not a missing fragment or generated changelog.

The reference point is the last release **on the branch you target**: a fix branched from
a `maint/` branch describes its change relative to that branch's most recent release,
which may be older than the most recent release on `main`.

### 6. Write through the selected source
**Zebra / Changie (ordinary PRs):**
- Use `changie new -j PROJECT -k KIND -b BODY`, one distinct change per fragment per
  project. Repeating `-j` creates one fragment for each project when wording and kind
  truly match. The breaking-addition exception requires separate `breaking` and `Added`
  fragments for each affected library.
- Body is the curated entry **without a leading Markdown `- ` or section heading**;
  Changie adds those. Include the real `([#N](PR_URL))` when available; never invent a
  link. Keep a single change together, including any necessary migration instructions.
- Shell-quote bodies safely: single quotes preserve backticks literally; do not put
  unescaped backticks inside double quotes, where the shell executes them. For bodies
  containing apostrophes or multiline text, prefer the interactive `changie new`
  prompts or an argument vector rather than composing unsafe shell strings.
- Prefer the CLI for creation. When reconciling an existing YAML fragment, preserve
  its parsed structure; inspect configuration and CLI-produced values rather than
  guessing how a kind is serialized.
- Leave `.changes/unreleased/` fragments unstaged for review. Ordinary PRs do **not**
  run `changie batch` or regenerate release changelogs.

**Direct Markdown repositories:**
- Merge curated entries into each matching `CHANGELOG.md` `[Unreleased]` section;
  keep existing unrelated entries and the heading even when empty. Route by audience
  in repos with separate operator and library changelogs.
- Leave edits unstaged. Follow [REFERENCE.md](REFERENCE.md) and local formatting rules.

In either workflow, review only the intended source diff. Keep source entries with
the change they describe so they travel through cherry-picks; do not commit or rewrite
history without authorization. For historical corrections, change only what that
release actually shipped: in Zebra edit its versioned `.changes/<project>/v<version>.md`
and run `changie merge`, never edit generated output. In direct Markdown repos edit the
released section itself. Later behavior belongs in new unreleased source entries.

## Release changelogs (the release-plz PR)

Compare from the **previous published release of each project** to the **exact final
release-PR head**, including its final `Cargo.toml`/`Cargo.lock` version changes:

```
zc --changelog <previous-release-ref> <final-release-pr-head>
```

If projects have different previous releases, run the appropriate comparisons and use
each project's results. A default branch-point diff misses unreleased changes; a run
against `main` misses release-PR dependency bumps. Repeat after the release owner changes
the version plan. This skill audits entries; it does not independently choose versions.

**Zebra:**
- The workflow pins Changie v1.25.2. release-plz chooses versions with
  `changelog_update = false` and `semver_check = false`; the batching script uses exact
  `v<version>` values from the release manifest, then runs `changie merge`.
  Never use `changie batch auto` to replace release-plz's version plan.
- `.github/scripts/batch-release-changelogs.sh` restores `.changes` from
  `base_revision` before batching. **Do not blindly rerun it over manual source
  curation:** it overwrites branch-local edits. After batching, curate
  `.changes/<project>/v<version>.md` on the release PR and run `changie merge`.
- The script adds a generic dependency entry only when a released project has **no**
  fragments. Existing fragments therefore do not prove dependency coverage. Audit
  zc's complete per-crate dependency list against the version file, replacing generic
  fallback wording where appropriate and adding missing entries without duplicates.
- Do not put consumed release entries back into `.changes/unreleased/` or patch the
  generated `CHANGELOG.md` files. Land missing source fragments on `main` before the
  final refresh when practical. After batching, keep version-file corrections on the
  release branch; preserve and reapply them if a later refresh regenerates the PR.

**Direct Markdown repositories:** curate the release PR's actual release sections in
their `CHANGELOG.md` files, following their release process.

For both workflows, skip dev-dependency-only entries and library dependency lists in
operator-facing `zebrad` entries. Review semantic changes and bug fixes from PRs too;
zc does not discover every consumer-visible change.

## `--check` mode: verify, do not write

A mode of **this skill**, not a flag on the `zc` binary: invoke `/zc --check`
(optionally with a PR or branch). Run ordinary zc analysis with the resolved range;
`--json` can provide structured API/dependency evidence. Do not pass `--check` to zc.
Do not create/edit fragments, batch, merge, change changelogs, stage, commit, or post
PR comments in this mode.

1. Record the exact baseline/head used for analysis. Use the same range for source
   comparison, with the release-specific per-project baselines above when applicable.
2. Collect **branch-specific net entries**, not every existing entry:
   - **Zebra ordinary PR:** inspect added, modified, renamed, and deleted
     `.changes/unreleased/*.yaml` at both refs. Parse complete YAML documents
     (including multiline bodies), then compare their decoded project/kind/body.
     Match project keys through `.changie.yaml` before comparing with zc's crate.
     Exclude unchanged content even if filenames changed; account for edits and
     deletions rather than grepping only added YAML lines. A removed or corrected
     stale entry is not a new undocumented API, and updating an unreleased addition
     must be judged by the net state from step 5, not an intermediate rename.
   - **Zebra release PR:** inspect the planned projects' target
     `.changes/<project>/v<version>.md` source entries, plus relevant fragments not
     yet batched. Treat consumed fragments as moved into version files, not deleted
     documentation or duplicate entries. Exclude unrelated historical versions and
     unchanged fragments outside this release. Never use generated changelogs as
     check input or run batching to make the comparison.
   - **Direct Markdown:** compare the corresponding `[Unreleased]` or release-section
     source entries at both refs, accounting for edits/deletions as well as additions.
     Do not compare the whole unchanged section against a branch-only API diff.
3. Compare API identifiers and dependency evidence with the curated net entries.
   Recognize identifiers in curated migration prose and grouped entries; do not treat
   every backticked argument/type as a separately claimed public API. Treat Zebra's
   required `breaking`/`Added` pair as intentional coverage, not a duplicate.
4. Report **Listed but not public** and **Public but undocumented**, with source paths
   and project keys, plus missing dependency coverage or wrong project/kind placement.
   Reconcile unreleased additions/renames/removals before classifying discrepancies.
   Flag prose/behavioral claims needing manual review rather than pretending zc can
   verify them. An analysis error is an incomplete check, never a pass.

Empty discrepancy lists establish only the checked API/dependency coverage. Inspect the
PR separately for bug fixes and semantic changes with unchanged signatures, operator
impact, PR links, and source entries accompanying the changes they describe.

## Notes
- Library entries are terse and code-pathed; operator entries use plain, user-facing
  descriptions. Both are source entries, even when the final destination is a changelog.
- zc's clean-tree default uses a branch point, so later parent-branch commits do not
  pollute an ordinary PR diff. Always override that default for release comparisons.
