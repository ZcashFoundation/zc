# Changelog example: ziff draft to curated entries

One illustrative pass through [SKILL.md](SKILL.md) for a single crate. The names are
made up; only the shape matters.

## ziff --changelog output (the draft)

```
## example-crate

### Added
- `config::Limits::max_connections`
- `network::Peer::Inbound`

### Changed
- `Client::connect(&self) -> Session` -> `Client::connect(&self, opts: Opts) -> Session`
```

## Reasoning

- `network::Peer::Inbound` is a new variant. If `Peer` is not `#[non_exhaustive]`,
  adding it breaks downstream exhaustive `match`es, so it moves to Breaking Changes
  (step 3).
- `config::Limits::max_connections` is a new public field. If `Limits` is
  constructed with a struct literal and is not `#[non_exhaustive]`, the new field
  breaks that literal, so it also moves to Breaking Changes (step 3).
- The `Client::connect` signature change is a `Changed`; render it as prose (step 4
  and REFERENCE.md), not as a raw signature dump.

## Curated `[Unreleased]` entries

```
### Breaking Changes
- `network::Peer` gained an `Inbound` variant, so exhaustive matches need a new arm
  (breaking; needs a major version bump).
- `config::Limits` gained a `max_connections` field, so struct literals must set it
  (breaking; needs a major version bump).

### Changed
- `Client::connect` now takes an `Opts` argument.
```

If `Peer` or `Limits` were `#[non_exhaustive]`, those two would stay under `### Added`
instead, because downstream code could not break on them.
