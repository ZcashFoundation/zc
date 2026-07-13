# Changelog example: zc draft to curated entries

One illustrative pass through [SKILL.md](SKILL.md) for a single crate. The names are
made up; only the shape matters.

## zc --changelog output (the draft)

```
## example-crate

### Added
- `config::Limits::max_connections`
- `network::Peer::Inbound`

### Changed
- `Client::connect(&self) -> Session` -> `Client::connect(&self, opts: Opts) -> Session`
```

## Reasoning

- `network::Peer::Inbound` is a new enum variant. lrz lists new variants under
  `### Added` even when they force a major bump (see `TxVersion::V6` in
  zcash_primitives) — it does not hoist them into a breaking section. It stays under
  `### Added`; the crate's major version bump records the break.
- `config::Limits::max_connections` is a new public field. Same rule: it stays under
  `### Added`.
- The `Client::connect` signature change is a `Changed`; render it as prose (step 3
  and REFERENCE.md), not as a raw signature dump.

## Curated `[Unreleased]` entries

```
### Added
- `config::Limits::max_connections`
- `network::Peer::Inbound`

### Changed
- `Client::connect` now takes an `Opts` argument.
```

Whether `Peer`/`Limits` are `#[non_exhaustive]` does not change placement — both
entries stay under `### Added`. It changes only whether the release needs a major
version bump, which the version-bump step decides.
