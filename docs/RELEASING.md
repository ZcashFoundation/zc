# Releasing zc

This runbook describes how maintainers publish a tagged release of zc. A tag
marks a reviewed commit on `main`; consumers install the CLI from it
(`cargo install --git … --tag`) and pin the action to its full commit SHA
(see [SECURITY.md](../SECURITY.md)).

## What protects a release

- The `main` ruleset requires a pull request with one approval, the CI checks,
  and linear history, so every commit on `main` was reviewed and green.
- The `release-tags` ruleset stops `v*` tags from being deleted, moved, or
  force-pushed. Repository admins can bypass it.
- Tags are created only by the "Release" workflow, from `main`, after it checks
  that the commit already names the version being released.

## Release steps

1. Open a `chore: release vX.Y.Z` pull request that bumps `version` in
   `Cargo.toml` and `Cargo.lock` and points the README's action pin
   (`ZcashFoundation/zc@vX.Y.Z`) and install command (`--tag vX.Y.Z`) at the
   new tag. Merge it once CI is green and it is approved.
2. Run the "Release" workflow (`workflow_dispatch` on `main`) with the version
   input in `vX.Y.Z` form. It checks the version format, that it runs on `main`,
   and that `Cargo.toml` and the README name that version. It then creates an
   annotated tag on the release commit and publishes a GitHub Release with
   generated notes.
3. Edit the release notes into a short summary of what changed for CLI and
   action users, including the install command and an action example.
4. Check that `cargo install --git https://github.com/ZcashFoundation/zc --tag vX.Y.Z --locked`
   reports the new version.

## Version policy

Use SemVer. Before v1.0.0, a new flag, input, or output bumps the minor
version, and so does a breaking change to the CLI, the JSON report, exit codes,
or action inputs and outputs. Call out breaking changes in the release notes.
Changing the pinned nightly or cargo-public-api version invalidates consumers'
rustdoc JSON caches; mention it in the release notes.
