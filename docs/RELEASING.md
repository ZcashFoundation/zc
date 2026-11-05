# Releasing zc

This runbook describes how maintainers publish a tagged release of zc from a
reviewed commit on `main`. Consumers install the CLI from a tag
(`cargo install --git … --tag`) and pin the action to its full commit SHA
(see [SECURITY.md](../SECURITY.md)). The process below is not an authorization
boundary: current repository rules do not ensure that every release tag was
created by this process.

## What protects a release

- For changes that do not bypass it, the `main` ruleset requires a pull request
  with one approval, the CI checks, and linear history. Repository admins can
  bypass it, so membership in `main` alone does not prove review or passing CI.
- The `release-tags` ruleset restricts updates and deletion of `v*` tags.
  Repository admins can bypass it. It does not restrict tag creation or require
  a tag's target to be on `main`.
- The "Release" workflow checks its branch, version input, and the release
  commit's version before creating a tag. These checks govern that workflow
  only; another actor with tag-creation permission can create a tag without
  them. Its `GITHUB_TOKEN` is not a dedicated release identity.

### Prerequisite for release-only tag creation

Enforcing workflow-only creation requires an administrator-provisioned,
dedicated release identity (for example, a GitHub App installation), credentials
available only to the release job, and a tag-creation ruleset that permits that
identity while blocking other creators. Any administrator bypass remains an
explicit exception. The release workflow must use that identity, not the shared
Actions identity, and the allow/deny behavior must be verified before claiming
enforcement. No such identity or creation restriction is configured by this
runbook; do not enable a restriction that prevents the existing release job from
creating tags before this prerequisite is in place.

## Release steps

1. Open a `chore: release vX.Y.Z` pull request that bumps `version` in
   `Cargo.toml` and `Cargo.lock` and points the README's action pin
   (`ZcashFoundation/zc@vX.Y.Z`) and install command (`--tag vX.Y.Z`) at the
   new tag. Merge it once CI is green and it is approved.
2. Verify the chosen commit is on `main`, was reviewed, and passed the required
   CI checks; admin bypass means these facts cannot be inferred from the branch
   alone. Run the "Release" workflow (`workflow_dispatch` on that commit on
   `main`) with the version input in `vX.Y.Z` form. It checks the version format,
   that it runs on `main`, and that `Cargo.toml` and the README name that version.
   It then creates an annotated tag on the selected commit and publishes a
   GitHub Release with generated notes.
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
