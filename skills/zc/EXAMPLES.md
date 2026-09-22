# Changelog example: zc draft to curated entries

Two repository-specific passes through [SKILL.md](SKILL.md). Item names below are
illustrative; project/kind keys in the Zebra commands are real.

## Generic direct Markdown repository

This repository uses Keep a Changelog without an additional breaking section.
Do not apply this placement or writer to Zebra.

### zc --changelog output (the draft)

```
## example-crate

### Added
- `config::Limits::max_connections`
- `network::Peer::Inbound`

### Changed
- `Client::connect(&self) -> Session` -> `Client::connect(&self, opts: Opts) -> Session`
```

### Reasoning

- `network::Peer::Inbound` is a new enum variant. In this repository it stays under
  `Added`, even if it breaks exhaustive matches; the release process decides the
  version bump. A repository-specific breaking section would override this rule.
- `config::Limits::max_connections` is a new public field. The same generic placement
  rule keeps it under `Added`.
- Render the `Client::connect` signature change as `Changed` prose, not a raw dump.

### Curated `[Unreleased]` entries

```
### Added
- `config::Limits::max_connections`
- `network::Peer::Inbound`

### Changed
- `Client::connect` now takes an `Opts` argument.
```

Whether `Peer`/`Limits` are `#[non_exhaustive]` changes compatibility, not this
repository's placement. Merge these entries into its direct `CHANGELOG.md` source.

## Zebra: curated draft to fragments

Run from a Zebra checkout with Changie installed (the release workflow pins v1.25.2).
These are runnable examples with illustrative bodies, not claims about a current PR.
Replace the bodies with the actual change before using them:

```sh
changie new -j zebrad -k Changed \
  -b 'Mempool now rejects transactions with non-standard transparent scripts.'
changie new -j zebra-chain -k breaking \
  -b 'Added `transaction::Transaction::V7`; downstream exhaustive matches must handle it.'
changie new -j zebra-chain -k Added \
  -b '`transaction::Transaction::V7`'
```

Each command writes a fragment in `.changes/unreleased/`. The first is operator-facing
`Changed`, not `Fixed`, because observable behavior changes. The hypothetical library
addition requires **both** `breaking` and `Added`, unlike the generic example above.
`breaking` renders as `Breaking Changes`; let the CLI serialize YAML kinds.
Single quotes preserve backticks literally, and bodies have no leading `- `.
Append a verified `([#N](PR_URL))` when the PR is known; do not invent one.
For identical changes in several projects, repeat `-j`, for example
`-j zebra-chain -j zebra-state`; Changie creates one fragment per project.

Ordinary PRs stop at fragments: no batching and no hand-edited root/crate changelogs.
Reconcile an existing fragment instead of duplicating it; removing an unreleased
addition also removes its source entry.

### Release and check variants

- **Zebra release:** compare `zc --changelog <previous-release-ref> <final-release-pr-head>`
  for each project's previous published release. After the release workflow consumes
  fragments, curate `.changes/<project>/v<version>.md` and run `changie merge`.
  release-plz owns the version plan; never substitute `changie batch auto`.
  Do not rerun the batching script over curation: it restores `.changes` from its base.
  Audit zc's dependency list even when an existing fragment prevented the script's
  generic dependency fallback.
- **`/zc --check`:** skill-only and read-only, never `zc --check`. Parse complete
  branch-specific net fragment entries at both refs, including edits/deletions and
  project keys. On release branches include the planned version files holding consumed
  entries, without double-counting those fragments. Exclude unchanged unrelated sources
  and generated changelogs; direct Markdown repos compare net section entries instead.
