# Changelog formatting reference

Mechanical rules for turning ziff's draft bullets into clean entries. The decision
flow (sections, breaking changes, where entries come from) is in
[SKILL.md](SKILL.md); this file covers only wording and layout.

## Periods
- No trailing period on a pure-identifier bullet, an `impl ... for ...` bullet, or a
  brace-group bullet.
- A trailing period on prose bullets and parenthetical glosses.

## `### Changed` entries
ziff prints raw `old -> new` signatures. Convert each to terse prose: a lead-in
ending in `:` ("The following now take an additional `foo: Bar`:") with the affected
items as 2-space sub-bullets, rather than dumping signatures. This is the main thing
you add by hand; ziff cannot infer it.

## Folding over-granular items
A struct variant's fields (for example `Foo::Bar::field`) belong inside the variant
(`Foo::Bar { field }`), not as their own bullets.

## Width
Wrap at about 100 chars with a 2-space hanging indent. Brace groups stay inline when
they fit; ziff already breaks the over-wide ones onto a `Type:` header with 2-space
sub-bullets.

## Joining
Do not join independent items with "X, with Y"; list them as separate bullets.
