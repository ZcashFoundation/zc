# Changelog formatting reference

Mechanical rules for curating zc's Markdown draft. [SKILL.md](SKILL.md) owns source
selection, reconciliation, release curation, and read-only `/zc --check`.

## Source and sections
- **Zebra:** ordinary PRs write `.changes/unreleased/` YAML through
  `changie new -j PROJECT -k KIND -b BODY`. `.changie.yaml` maps `zebrad` to the
  root changelog and library projects to their crate changelogs. Never hand-edit
  generated `CHANGELOG.md` files. Body contains no leading Markdown bullet or heading;
  Changie adds them. Include a verified PR link when available.
- Zebra CLI kinds, in display order: `breaking` (renders `Breaking Changes`),
  `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`. Prefer CLI-produced
  fragments; do not infer the YAML `kind` value from a display label.
- Zebra uses one entry per distinct change/project, in the highest-impact category,
  **except breaking library additions require both `breaking` and `Added` fragments**.
  `Fixed` is only for invisible fixes; changed output or behavior uses `Changed`
  unless a higher-priority category applies.
- **Direct Markdown / generic Keep a Changelog:** without a repo-specific breaking
  section, use `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.
  Breaking removals go under `Removed`, signature/behavior/type changes under
  `Changed`, and additions under `Added`. A rare bold **BREAKING CHANGES** prefix
  is optional only where that repository uses it.

Single-quote CLI bodies to preserve backticks literally; unescaped backticks inside
double quotes execute shell commands. For apostrophes or multiline bodies, use
interactive `changie new` prompts or an argument vector instead of unsafe interpolation.

For Zebra releases and historical corrections, curate
`.changes/<project>/v<version>.md` and run `changie merge`. Versions come from
release-plz, never an independent `changie batch auto` decision. The release batching
script restores `.changes` from its base revision, so do not rerun it over manual
curation. Audit dependency entries with zc even when fragments already existed.

`/zc --check` reads branch-specific net source entries without writing: parse whole
fragments at both refs, including edits/deletions and project keys, not added YAML
lines or unchanged entries. On a release branch, account for consumed fragments in
the planned version files; compare previous published releases to the final release-PR
head. Never use generated changelogs as check input. Direct Markdown repos compare
their own net section entries instead. See SKILL.md for the full comparison rules.

## Periods
- No trailing period on a pure-identifier bullet, an `impl ... for ...` bullet, or a
  brace-group bullet.
- A trailing period on prose bullets and parenthetical glosses.

## `### Added` entries
For library API additions, prefer an item path; its `rustdoc` carries the explanation.
Add a gloss where needed — a feature flag or `(non-exhaustive)` marker — after a colon,
as prose with a trailing period. Add the PR link when available. Rendered example:

```
- `error::ParseError::NumberMissingDigits` (non-exhaustive): emitted internally by
  `Value::parse` when a `Number` candidate has no mantissa digits.
```

## `### Changed` entries
zc prints raw `old -> new` signatures. Convert each to terse prose stating the new
behavior, not a signature dump: "`Foo::bar` now takes a `NonZeroU8` instead of a
`u8`." In Zebra, a breaking signature change uses `breaking`, not `Changed`.
For several items sharing one change, use a lead-in ending in `:` ("The following
now take an additional `foo: Bar`:") with 2-space sub-bullets. This remains one
distinct entry/fragment, not separate unrelated changes. zc cannot infer the prose.

Dependency and toolchain lines follow fixed forms: "Migrated to `orchard 0.15`,
`zcash_protocol 0.10.0`." and "MSRV is now 1.88".

## Folding over-granular items
A struct variant's fields (for example `Foo::Bar::field`) belong inside the variant
(`Foo::Bar { field }`), not as their own bullets.

## Width
Wrap Markdown at about 100 chars with a 2-space hanging indent. Brace groups stay
inline when they fit; zc breaks over-wide ones onto a `Type:` header with 2-space
sub-bullets. For fragment creation, pass the body to Changie; do not manually wrap
or serialize YAML to imitate the generated layout.

## Joining
Do not join independent items with "X, with Y"; list them as separate bullets.

## Never-released crates
Follow repository policy. For direct Markdown repos using the initial-release
convention, a crate with no prior release has one line and no itemized API:

```
## [0.1.0] - Tue 31 March 2026

- Initial release!
```

This is not an instruction to edit a generated Zebra changelog or choose its version.
