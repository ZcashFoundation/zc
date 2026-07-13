# Changelog formatting reference

Mechanical rules for turning zc's draft bullets into clean entries. The decision
flow (sections, breaking changes, where entries come from) is in
[SKILL.md](SKILL.md); this file covers only wording and layout.

## Sections
Order (Keep a Changelog): `### Added`, `### Changed`, `### Deprecated`, `### Removed`,
`### Fixed`, `### Security`. There is **no `### Breaking Changes` section** — lrz does
not use one. Breaking items sit in `### Removed` (removals) or `### Changed`
(signature/behavior/type changes); an exceptionally disruptive one may open its
bullet with a bold **BREAKING CHANGES** marker, as lrz does rarely.

## Periods
- No trailing period on a pure-identifier bullet, an `impl ... for ...` bullet, or a
  brace-group bullet.
- A trailing period on prose bullets and parenthetical glosses.

## `### Changed` entries
zc prints raw `old -> new` signatures. Convert each to terse prose stating the new
behavior, not a signature dump: "`Foo::bar` now takes a `NonZeroU8` instead of a
`u8`." For several items sharing one change, use a lead-in ending in `:` ("The
following now take an additional `foo: Bar`:") with the affected items as 2-space
sub-bullets. This is the main thing you add by hand; zc cannot infer it.

Dependency and toolchain lines follow fixed forms: "Migrated to `orchard 0.15`,
`zcash_protocol 0.10.0`." and "MSRV is now 1.88".

## Folding over-granular items
A struct variant's fields (for example `Foo::Bar::field`) belong inside the variant
(`Foo::Bar { field }`), not as their own bullets.

## Width
Wrap at about 100 chars with a 2-space hanging indent. Brace groups stay inline when
they fit; zc already breaks the over-wide ones onto a `Type:` header with 2-space
sub-bullets.

## Joining
Do not join independent items with "X, with Y"; list them as separate bullets.
