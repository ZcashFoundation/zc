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
Release tags are created by the process in [docs/RELEASING.md](docs/RELEASING.md)
and are protected from being moved or deleted.

## What zc runs

The action builds zc from the pinned commit with its committed `Cargo.lock`,
and installs a pinned Rust nightly and `cargo-public-api` with checksum
verification. zc then builds rustdoc for the analysed workspace at both refs,
which runs that workspace's build scripts and procedural macros. Run it only
where you would already build that code, with the least workflow permissions
the job needs.
