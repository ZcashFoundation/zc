# Security

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Follow the Zcash
Foundation's [security policy](https://zfnd.org/security/) to report it
privately, and include the affected zc version or commit, how you run it (CLI
or action, and the workflow permissions), and the smallest reproduction you can
provide.

## Supported versions

Before v1.0.0, only the latest release receives security fixes. Pin the action
to the full commit SHA of a release tag and let Dependabot propose updates.
The intended release process is in [docs/RELEASING.md](docs/RELEASING.md).
The `v*` tag ruleset restricts updates and deletion, with an administrator bypass;
it does not authorize tag creation or guarantee that a tag names a reviewed
commit on `main`. The `main` review and CI rules also allow administrator bypass.
The release workflow validates its own inputs and target, but other permitted
tag creators are not constrained by those checks. Enforcing release-only
creation requires the dedicated release identity and creation policy described
in the runbook; that prerequisite is not currently provided by the workflow's
shared `GITHUB_TOKEN`.

## What zc runs

The action builds zc from the pinned commit with its committed `Cargo.lock`,
and installs a pinned Rust nightly and `cargo-public-api` with checksum
verification. zc then builds rustdoc for the analysed workspace at both refs,
which runs that workspace's build scripts and procedural macros. Run it only
where you would already build that code, with the least workflow permissions
the job needs.
