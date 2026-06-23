# ziff

Diff the **public API** and **dependencies** of a Rust workspace's crates
between two git refs — and flag the changes that are breaking for downstream
consumers.

`ziff` wraps [`cargo public-api`](https://github.com/cargo-public-api/cargo-public-api)
and adds workspace-wide dependency, lockfile, and const/value diffing on top, so
a single command tells you whether a branch breaks your published surface.

## What it reports

1. **Workspace dependency diff** — each dep classified by the strongest kind it
   is used with (runtime > build > dev); consumer-visible breaking changes
   (runtime removals, major bumps, lost features) are highlighted.
2. **Transitive `Cargo.lock` diff** (`--with-lock`) — transitive version changes,
   each annotated with the direct deps that pull it in.
3. **Per-crate public-API diff** (`cargo public-api`) — removed / changed / added
   items per workspace crate.
4. **Const/static value & doc-comment diff** (`--with-values`) — catches changes
   `cargo public-api` can't see, since it compares signatures only.

It ends with a `BREAKING` / `ERROR` / `OK` verdict (and a `--json` mode for CI).

## Install

First install the prerequisites (see [Requirements](#requirements)):

```sh
cargo install cargo-public-api    # required; jq is also required
```

Then get `ziff` itself — it's a single Bash script, so either:

```sh
# Install onto your PATH
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/ziff/main/ziff \
  -o ~/.local/bin/ziff && chmod +x ~/.local/bin/ziff

# …or run a one-off without installing
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/ziff/main/ziff | bash -s -- main
```

In CI, the download-then-run form is the most robust:

```sh
curl -fsSL https://raw.githubusercontent.com/ZcashFoundation/ziff/main/ziff -o ziff
chmod +x ziff
./ziff "$BASE".."$HEAD" --json
```

> When run via `curl … | bash`, `--help` shows only a short usage (the script
> can't re-read its own source from a pipe); install it to a file for the full
> reference.

## Usage

```sh
ziff                      # dirty tree: HEAD -> working tree; clean: parent-branch -> HEAD
ziff main                 # compare against main (-> working tree if dirty, else HEAD)
ziff v4.1.0 v4.2.0        # compare two arbitrary refs
ziff --fetch              # fetch the upstream main tip first, then diff against it
ziff --with-lock          # include the transitive Cargo.lock diff
ziff --with-values main    # also flag const/static value + doc changes
ziff --json main          # machine-readable output for CI
```

Run `ziff --help` for the full option and output reference.

### `--fetch`

`--fetch[=<remote>]` pins the baseline to the *current* tip of a remote branch
instead of a possibly-stale local one, so a comparison isn't fooled by upstream
having moved on. If the fetch can't run (offline, or no SSH auth in a
non-interactive shell), it falls back to the last-synced `<remote>/<branch>`
tracking ref and warns with that ref's commit age.

## Requirements

- Bash 4+ (associative arrays)
- [`cargo-public-api`](https://github.com/cargo-public-api/cargo-public-api)
- `jq`
- a `nightly` toolchain — only for `--with-values`

## Exit codes

- `0` — no breaking changes (additive changes are fine)
- `1` — breaking changes detected, `cargo public-api` failed for a crate, or a
  runtime error

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

---

Originally developed in the [Zebra](https://github.com/ZcashFoundation/zebra)
repository as `zebra-utils/check-api`.
