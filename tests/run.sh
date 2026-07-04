#!/usr/bin/env bash
# Integration tests for ziff.
#
# Each test builds a throwaway single-crate git repo, mutates its public API
# (or dependencies), runs ziff against two refs, and asserts the verdict and
# exit code. We assert *behavior* (verdict / exit code / a key item name), not
# exact `cargo public-api` output — that text shifts with the rustc version.
#
# Requires the same tools ziff does: cargo-public-api, jq, cargo, a Rust
# toolchain (with a nightly available, which cargo-public-api uses for rustdoc
# JSON). Run from anywhere:  tests/run.sh
set -uo pipefail

ZIFF=${ZIFF:-"$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/ziff"}
pass=0
fail=0

ok()  { printf '  \033[32mok\033[0m   %s\n' "$1"; pass=$((pass + 1)); }
bad() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; fail=$((fail + 1)); }

assert_eq()       { if [ "$1" = "$2" ]; then ok "$3"; else bad "$3 (got '$1', want '$2')"; fi; }
assert_contains() { case "$1" in *"$2"*) ok "$3" ;; *) bad "$3 (output missing: $2)" ;; esac; }

worktree_count() {
    git -C "$1" worktree list --porcelain | grep -c '^worktree '
}

assert_repo_unchanged() { # $1=repo  $2=head  $3=status  $4=branch  $5=worktrees  $6=label
    local repo=$1 before_head=$2 before_status=$3 before_branch=$4 before_worktrees=$5 label=$6
    local after_head after_status after_branch after_worktrees
    after_head=$(git -C "$repo" rev-parse HEAD)
    after_status=$(git -C "$repo" status --porcelain)
    after_branch=$(git -C "$repo" branch --show-current)
    after_worktrees=$(worktree_count "$repo")

    assert_eq "$after_head" "$before_head" "$label: HEAD unchanged"
    assert_eq "$after_status" "$before_status" "$label: status unchanged"
    assert_eq "$after_branch" "$before_branch" "$label: branch unchanged"
    assert_eq "$after_worktrees" "$before_worktrees" "$label: no stale worktrees"
}

# new_repo <lib.rs contents> -> prints the repo dir.
# Creates a single-crate cargo lib in a fresh git repo with a committed lockfile.
new_repo() {
    local d
    d=$(mktemp -d)
    git -C "$d" init -q
    git -C "$d" config user.email t@t
    git -C "$d" config user.name t
    git -C "$d" config commit.gpgsign false
    printf '/target\n' >"$d/.gitignore"
    cat >"$d/Cargo.toml" <<'EOF'
[package]
name = "ziff_fixture"
version = "0.1.0"
edition = "2021"
EOF
    mkdir -p "$d/src"
    printf '%s\n' "$1" >"$d/src/lib.rs"
    ( cd "$d" && cargo generate-lockfile -q ) >/dev/null 2>&1
    git -C "$d" add -A
    git -C "$d" commit -qm base
    printf '%s' "$d"
}

# commit_lib <repo> <lib.rs contents> <msg> -> prints the new HEAD sha.
commit_lib() {
    printf '%s\n' "$2" >"$1/src/lib.rs"
    git -C "$1" add -A
    git -C "$1" commit -qm "$3"
    git -C "$1" rev-parse HEAD
}

echo "ziff integration tests ($ZIFF)"

# 1) Removing a public item is breaking.
repo=$(new_repo 'pub fn foo() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" 'pub fn bar() {}' 'swap foo -> bar')
before_head=$(git -C "$repo" rev-parse HEAD)
before_status=$(git -C "$repo" status --porcelain)
before_branch=$(git -C "$repo" branch --show-current)
before_worktrees=$(worktree_count "$repo")
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 ); rc=$?
assert_eq "$rc" 1 "removed pub fn: exit 1"
assert_contains "$out" "BREAKING" "removed pub fn: BREAKING verdict"
assert_contains "$out" "foo" "removed pub fn: names the removed item"
assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "removed pub fn: repository isolation"
rm -rf "$repo"

# 2) A pure addition is OK.
repo=$(new_repo 'pub fn foo() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn foo() {}\npub fn added() {}' 'add fn')
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 ); rc=$?
assert_eq "$rc" 0 "additive only: exit 0"
assert_contains "$out" "OK" "additive only: OK verdict"

# 3) Changing only a private item is no public-API change.
head2=$(commit_lib "$repo" $'pub fn foo() {}\npub fn added() {}\nfn helper() {}' 'add private fn')
out=$( cd "$repo" && "$ZIFF" "$head" "$head2" 2>&1 ); rc=$?
assert_eq "$rc" 0 "private-only change: exit 0"
assert_contains "$out" "No public API changes" "private-only change: reported as no change"
rm -rf "$repo"

# 4) --json emits a valid document with the expected shape and verdict.
repo=$(new_repo 'pub fn foo() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" 'pub fn bar() {}' 'swap')
json=$( cd "$repo" && "$ZIFF" --json "$base" "$head" 2>/dev/null ); rc=$?
assert_eq "$rc" 1 "--json: breaking changes exit 1"
if printf '%s' "$json" | jq -e 'has("totals") and has("crates") and .verdict == "breaking"' >/dev/null 2>&1; then
    ok "--json: valid shape, verdict=breaking"
else
    bad "--json: expected verdict:breaking, got: $(printf '%s' "$json" | head -c 120)"
fi
rm -rf "$repo"

# 4b) A crate that fails to document exits 2 and carries structured error data.
repo=$(new_repo 'pub fn foo() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'compile_error!("ziff fixture build failure");\npub fn foo() {}' 'break docs')
before_head=$(git -C "$repo" rev-parse HEAD)
before_status=$(git -C "$repo" status --porcelain)
before_branch=$(git -C "$repo" branch --show-current)
before_worktrees=$(worktree_count "$repo")
json=$( cd "$repo" && "$ZIFF" --json "$base" "$head" 2>/dev/null ); rc=$?
assert_eq "$rc" 2 "--json: analysis error exits 2"
if printf '%s' "$json" | jq -e '
    .verdict == "error" and
    .totals.error_crates == 1 and
    .crates[0].status == "error" and
    .crates[0].error.stage == "head_build" and
    (.crates[0].error.stderr | contains("ziff fixture build failure")) and
    (.crates[0].error.hint | length > 0)
' >/dev/null 2>&1; then
    ok "--json: structured crate error includes stage, stderr, hint"
else
    bad "--json: missing structured error fields, got: $(printf '%s' "$json" | head -c 240)"
fi
assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "--json error: repository isolation"
before_head=$(git -C "$repo" rev-parse HEAD)
before_status=$(git -C "$repo" status --porcelain)
before_branch=$(git -C "$repo" branch --show-current)
before_worktrees=$(worktree_count "$repo")
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 ); rc=$?
assert_eq "$rc" 2 "human error: analysis error exits 2"
assert_contains "$out" "stage: head_build" "human error: shows failing stage"
assert_contains "$out" "ziff fixture build failure" "human error: shows stderr tail"
assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "human error: repository isolation"
before_head=$(git -C "$repo" rev-parse HEAD)
before_status=$(git -C "$repo" status --porcelain)
before_branch=$(git -C "$repo" branch --show-current)
before_worktrees=$(worktree_count "$repo")
changelog_stdout_file=$(mktemp)
changelog_stderr_file=$(mktemp)
( cd "$repo" && "$ZIFF" --changelog "$base" "$head" >"$changelog_stdout_file" 2>"$changelog_stderr_file" ); rc=$?
changelog_stdout=$(cat "$changelog_stdout_file")
changelog_stderr=$(cat "$changelog_stderr_file")
rm -f "$changelog_stdout_file" "$changelog_stderr_file"
assert_eq "$rc" 2 "--changelog error: analysis error exits 2"
assert_eq "$changelog_stdout" "" "--changelog error: stdout empty"
assert_contains "$changelog_stderr" "stage: head_build" "--changelog error: stderr shows failing stage"
assert_contains "$changelog_stderr" "ziff_fixture" "--changelog error: stderr names failing crate"
assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "--changelog error: repository isolation"
rm -rf "$repo"

# 4c) Usage errors use a distinct code.
out=$( "$ZIFF" --definitely-not-a-ziff-option 2>&1 ); rc=$?
assert_eq "$rc" 64 "usage error: exit 64"
assert_contains "$out" "unknown option" "usage error: explains the option failure"

# 5) The default baseline is the BRANCH POINT (merge-base with the parent), not
#    the parent's tip: API changes merged onto the parent *after* we branched
#    must not leak into our diff. Branch `feature` off `main`, add our own item,
#    then advance `main` with an unrelated item; `ziff` (no args) on `feature`
#    must show our addition and ignore main's post-branch change.
repo=$(new_repo 'pub fn foo() {}')
git -C "$repo" branch -M main
git -C "$repo" checkout -q -b feature
commit_lib "$repo" $'pub fn foo() {}\npub fn feature_fn() {}' 'feature work' >/dev/null
git -C "$repo" checkout -q main
commit_lib "$repo" $'pub fn foo() {}\npub fn upstream_fn() {}' 'upstream work after branch' >/dev/null
git -C "$repo" checkout -q feature
out=$( cd "$repo" && "$ZIFF" 2>&1 ); rc=$?
assert_contains "$out" "feature_fn" "merge-base default: shows the branch's own addition"
case "$out" in
    *upstream_fn*) bad "merge-base default: leaked the parent's post-branch change (upstream_fn)" ;;
    *) ok "merge-base default: excludes the parent's post-branch change" ;;
esac
# And the header should announce the merge-base baseline, not a branch tip.
assert_contains "$out" "merge-base(main, HEAD)" "merge-base default: labels the baseline"
rm -rf "$repo"

# 5b) The default baseline ignores a branch's OWN remote: a feature branch pushed
#     with `git push -u` tracks `origin/<itself>`, which is useless to diff
#     against (same commit). Detection must fall through to `main`, not compare
#     the branch to itself (which would yield an empty diff).
repo=$(new_repo 'pub fn foo() {}')
git -C "$repo" branch -M main
git -C "$repo" checkout -q -b feature
commit_lib "$repo" $'pub fn foo() {}\npub fn feature_fn() {}' 'feature work' >/dev/null
git -C "$repo" checkout -q main
commit_lib "$repo" $'pub fn foo() {}\npub fn upstream_fn() {}' 'main advances after branch' >/dev/null
git -C "$repo" checkout -q feature
# simulate `git push -u`: feature tracks origin/feature, pointing at its own tip
git -C "$repo" remote add origin "$repo"
git -C "$repo" update-ref refs/remotes/origin/feature "$(git -C "$repo" rev-parse feature)"
git -C "$repo" config branch.feature.remote origin
git -C "$repo" config branch.feature.merge refs/heads/feature
out=$( cd "$repo" && "$ZIFF" 2>&1 ); rc=$?
assert_contains "$out" "merge-base(main, HEAD)" "self-upstream: baseline falls through to main"
assert_contains "$out" "feature_fn" "self-upstream: still shows the branch's own addition"
rm -rf "$repo"

# 6) Default grouping is a MODULE > TYPE > member hierarchy: items cluster under
#    their module, then under a type sub-header (tagged with its kind), with the
#    type prefix factored out of each member.
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub enum Color { Red, Green, Blue }' 'add Color enum')
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 )
if printf '%s\n' "$out" | grep -qE '^ +ziff_fixture$'; then
    ok "default: bare module header (no tag)"
else
    bad "default: expected a bare 'ziff_fixture' module header"
fi
if printf '%s\n' "$out" | grep -qE '^ +Color +\(enum\)$'; then
    ok "default: type sub-header with (enum) tag"
else
    bad "default: expected a 'Color  (enum)' sub-header"
fi
assert_contains "$out" "+ pub Red" "default: type prefix factored from variant"
if printf '%s\n' "$out" | grep -qF 'pub Color::Red'; then
    bad "default: variant should not repeat the Color:: prefix"
else
    ok "default: Color:: prefix not repeated on variants"
fi

# 6b) --by-type uses a flat type header (tagged); members keep the type prefix.
byt=$( cd "$repo" && "$ZIFF" --by-type "$base" "$head" 2>&1 )
if printf '%s\n' "$byt" | grep -qE '^ +ziff_fixture::Color +\(enum\)$'; then
    ok "--by-type: flat type header with (enum) tag"
else
    bad "--by-type: expected 'ziff_fixture::Color  (enum)' header"
fi
assert_contains "$byt" "+ pub Color::Red" "--by-type: members keep the type prefix"

# 6c) --flat keeps fully-qualified paths and emits no indented group header.
flat=$( cd "$repo" && "$ZIFF" --flat "$base" "$head" 2>&1 )
assert_contains "$flat" "pub ziff_fixture::Color::Red" "--flat: keeps fully-qualified paths"
if printf '%s\n' "$flat" | grep -qE '^ +(ziff_fixture|Color)(::[A-Za-z0-9_]+)*( +\([a-z]+\))?$'; then
    bad "--flat: should not emit a group header"
else
    ok "--flat: no group header"
fi
rm -rf "$repo"

# 6d) Kind from source: a pre-existing type whose declaration isn't in the diff
#     (only a method added) gets its real kind read from the head source — here a
#     trait, which neither the diff nor member inference could determine.
repo=$(new_repo $'pub trait Speak { fn hello(&self); }')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub trait Speak { fn hello(&self); fn bye(&self); }' 'add trait method bye')
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 )
if printf '%s\n' "$out" | grep -qE '^ +Speak +\(trait\)$'; then
    ok "default: pre-existing type kind read from head source (trait)"
else
    bad "default: expected a 'Speak  (trait)' sub-header from source lookup"
fi
rm -rf "$repo"

# 6e) Clustering: an enum with struct-variants interleaves variant items with
#     the variants' field items, yet must produce a single enum header (not a
#     duplicate per run).
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub enum E { A { x: u32 }, B { y: u32 } }' 'add enum with struct-variants')
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 )
nhdr=$(printf '%s\n' "$out" | grep -cE '^ +E +\(enum\)$')
assert_eq "$nhdr" "1" "clustering: enum header appears exactly once (no duplicate)"
rm -rf "$repo"

# 6f) Elision tolerates generics on the owning type: a member of `Wrap<u32>`
#     should be factored to a bare name, not `Wrap<u32>::get`.
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub struct Wrap<T>(pub T);\nimpl Wrap<u32> { pub fn get(&self) -> u32 { self.0 } }' 'add generic Wrap')
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 )
assert_contains "$out" "+ pub fn get(" "generics: type prefix with generic args is factored"
if printf '%s\n' "$out" | grep -qF 'Wrap<u32>::get'; then
    bad "generics: member should not keep the Wrap<u32>:: prefix"
else
    ok "generics: Wrap<u32>:: prefix factored from member"
fi
rm -rf "$repo"

# 6g) External-type bucketing: items whose path is in another crate (a trait
#     impl this crate adds to a foreign type) get a dedicated section.
ws=$(mktemp -d)
mkdir -p "$ws/dep/src" "$ws/ziff_fixture/src"
printf '/target\n' >"$ws/.gitignore"
cat >"$ws/Cargo.toml" <<'EOF'
[workspace]
members = ["dep", "ziff_fixture"]
resolver = "2"
EOF
cat >"$ws/dep/Cargo.toml" <<'EOF'
[package]
name = "dep"
version = "0.1.0"
edition = "2021"
EOF
echo 'pub struct Foo;' >"$ws/dep/src/lib.rs"
cat >"$ws/ziff_fixture/Cargo.toml" <<'EOF'
[package]
name = "ziff_fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
dep = { path = "../dep" }
EOF
printf 'pub trait Ext { fn tag(&self) -> u8; }\nimpl Ext for dep::Foo { fn tag(&self) -> u8 { 0 } }\n' >"$ws/ziff_fixture/src/lib.rs"
git -C "$ws" init -q
git -C "$ws" config user.email t@t
git -C "$ws" config user.name t
git -C "$ws" config commit.gpgsign false
( cd "$ws" && cargo generate-lockfile -q ) >/dev/null 2>&1
git -C "$ws" add -A
git -C "$ws" commit -qm base
base=$(git -C "$ws" rev-parse HEAD)
printf 'pub trait Ext { fn tag(&self) -> u8; fn name(&self) -> u8; }\nimpl Ext for dep::Foo { fn tag(&self) -> u8 { 0 } fn name(&self) -> u8 { 1 } }\n' >"$ws/ziff_fixture/src/lib.rs"
git -C "$ws" add -A
git -C "$ws" commit -qm head
head=$(git -C "$ws" rev-parse HEAD)
out=$( cd "$ws" && "$ZIFF" "$base" "$head" 2>&1 )
assert_contains "$out" "[trait impls on external types]" "external: foreign-type items get a dedicated section"
# The foreign item is bucketed under its real crate (`dep`), not as a module of
# the analyzed crate.
if printf '%s\n' "$out" | grep -qE '^ +dep$'; then
    ok "external: foreign item bucketed under its own crate path"
else
    bad "external: expected a 'dep' module header under the external section"
fi
rm -rf "$ws"

# 6h) --changelog emits librustzcash-style markdown: per-crate heading, ### Added,
#     items grouped under their type with bare member names.
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub struct Widget;\nimpl Widget { pub fn new() -> Self { Widget } pub fn run(&self) {} }' 'add Widget')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "## ziff_fixture" "--changelog: per-crate heading"
assert_contains "$out" "### Added" "--changelog: Added section"
assert_contains "$out" "- \`Widget::{new, run}\`" "--changelog: type members brace-grouped on one line"
rm -rf "$repo"

# 6h2) --changelog wraps an over-wide brace group lrz-style: when `Type::{...}`
#      would exceed the line budget, it breaks onto a `Type:` header with one
#      2-space-indented member per line instead of one long brace line.
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub struct Verifier;\nimpl Verifier { pub fn check_cross_address_disabled(&self) {} pub fn enforce_nullifier_uniqueness(&self) {} pub fn validate_ironwood_proof_size(&self) {} pub fn validate_orchard_value_balance(&self) {} }' 'add Verifier')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`Verifier\`:" "--changelog: over-wide group uses a type header"
assert_contains "$out" "  - \`validate_ironwood_proof_size\`" "--changelog: over-wide group members indented as sub-bullets"
case "$out" in
    *'Verifier::{'*) bad "--changelog: over-wide group should not stay on one brace line" ;;
    *) ok "--changelog: over-wide group is not kept inline" ;;
esac
rm -rf "$repo"

# 6i) --changelog documents per-crate dependency changes: an internal workspace
#     crate bump becomes a "dependency bumped to" line under ### Changed, and a
#     dropped dependency becomes a "dependency." line under ### Removed. Uses
#     versioned path deps so the requirement string actually changes (and so the
#     fixture resolves offline).
ws=$(mktemp -d)
mkdir -p "$ws/dep/src" "$ws/dep2/src" "$ws/ziff_fixture/src"
printf '/target\n' >"$ws/.gitignore"
cat >"$ws/Cargo.toml" <<'EOF'
[workspace]
members = ["dep", "dep2", "ziff_fixture"]
resolver = "2"
EOF
printf '[package]\nname = "dep"\nversion = "0.1.0"\nedition = "2021"\n' >"$ws/dep/Cargo.toml"
printf '[package]\nname = "dep2"\nversion = "0.1.0"\nedition = "2021"\n' >"$ws/dep2/Cargo.toml"
echo 'pub struct Foo;' >"$ws/dep/src/lib.rs"
echo 'pub struct Bar;' >"$ws/dep2/src/lib.rs"
cat >"$ws/ziff_fixture/Cargo.toml" <<'EOF'
[package]
name = "ziff_fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
dep = { path = "../dep", version = "0.1.0" }
dep2 = { path = "../dep2", version = "0.1.0" }
EOF
echo 'pub fn placeholder() {}' >"$ws/ziff_fixture/src/lib.rs"
git -C "$ws" init -q
git -C "$ws" config user.email t@t
git -C "$ws" config user.name t
git -C "$ws" config commit.gpgsign false
( cd "$ws" && cargo generate-lockfile -q ) >/dev/null 2>&1
git -C "$ws" add -A
git -C "$ws" commit -qm base
base=$(git -C "$ws" rev-parse HEAD)
# Bump `dep` to 0.2.0 (updating ziff_fixture's requirement) and drop `dep2`.
printf '[package]\nname = "dep"\nversion = "0.2.0"\nedition = "2021"\n' >"$ws/dep/Cargo.toml"
cat >"$ws/ziff_fixture/Cargo.toml" <<'EOF'
[package]
name = "ziff_fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
dep = { path = "../dep", version = "0.2.0" }
EOF
( cd "$ws" && cargo generate-lockfile -q ) >/dev/null 2>&1
git -C "$ws" add -A
git -C "$ws" commit -qm head
head=$(git -C "$ws" rev-parse HEAD)
out=$( cd "$ws" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`dep\` dependency bumped to \`0.2.0\`." "--changelog: internal dep bump under Changed"
assert_contains "$out" "- \`dep2\` dependency." "--changelog: dropped dep under Removed"
rm -rf "$ws"

# 6j) An *added* trait impl's associated item is grouped under its `impl Trait
#     for Self` header (the trait recovered from rustdoc, the Self generics from
#     the signature) rather than under a bare `Foo` type.
repo=$(new_repo $'pub trait IntoDisk { type Bytes; }\npub struct Foo<T>(pub T);')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub trait IntoDisk { type Bytes; }\npub struct Foo<T>(pub T);\nimpl IntoDisk for Foo<u32> { type Bytes = [u8; 48]; }' 'add IntoDisk impl')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`impl IntoDisk for Foo<u32>\`:" "--changelog: added assoc item grouped under impl header with Self generics"
assert_contains "$out" "- \`Bytes\`" "--changelog: assoc type shown as a bare member under the impl"
rm -rf "$repo"

# 6m) A *changed* item shows the before -> after signatures, not just its name —
#     a "Changed" entry has to say what changed.
repo=$(new_repo 'pub fn f() -> u8 { 0 }')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" 'pub fn f() -> u16 { 0 }' 'widen return type')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`fn f() -> u8\`" "--changelog: Changed shows the old signature"
assert_contains "$out" "→ \`fn f() -> u16\`" "--changelog: Changed shows the new signature after an arrow"
rm -rf "$repo"

# 6n) An MSRV (rust-version) bump is documented under ### Changed.
repo=$(mktemp -d)
git -C "$repo" init -q; git -C "$repo" config user.email t@t; git -C "$repo" config user.name t; git -C "$repo" config commit.gpgsign false
printf '/target\n' >"$repo/.gitignore"
printf '[package]\nname = "ziff_fixture"\nversion = "0.1.0"\nedition = "2021"\nrust-version = "1.70"\n' >"$repo/Cargo.toml"
mkdir -p "$repo/src"; echo 'pub fn f() {}' >"$repo/src/lib.rs"
( cd "$repo" && cargo generate-lockfile -q ) >/dev/null 2>&1
git -C "$repo" add -A; git -C "$repo" commit -qm base
base=$(git -C "$repo" rev-parse HEAD)
printf '[package]\nname = "ziff_fixture"\nversion = "0.1.0"\nedition = "2021"\nrust-version = "1.75"\n' >"$repo/Cargo.toml"
( cd "$repo" && cargo generate-lockfile -q ) >/dev/null 2>&1
git -C "$repo" add -A; git -C "$repo" commit -qm head
head=$(git -C "$repo" rev-parse HEAD)
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- MSRV is now 1.75." "--changelog: MSRV bump documented under Changed"
rm -rf "$repo"

# 6o) A trait implemented on several types is grouped by trait, lrz-style:
#     `impl Trait` for: + the types as sub-bullets.
repo=$(new_repo $'pub trait Marker {}\npub struct A;\npub struct B;')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub trait Marker {}\npub struct A;\npub struct B;\nimpl Marker for A {}\nimpl Marker for B {}' 'impl Marker on A and B')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`impl Marker\` for:" "--changelog: a trait on multiple types is grouped by trait"
rm -rf "$repo"

# 6p) A *removed* trait-impl method groups under its `impl Trait for Self` header,
#     using the base-ref trait map (the impl is gone at head, so the head map
#     can't attribute it).
repo=$(new_repo $'pub trait T { fn m(&self); }\npub struct S;\nimpl T for S { fn m(&self) {} }')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub trait T { fn m(&self); }\npub struct S;' 'remove impl T for S')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`impl T for S\`:" "--changelog: removed impl method grouped under its impl (base map)"
rm -rf "$repo"

# 6q) An impl with its own generic/lifetime params (`impl<'a> ... for ...`) is
#     recognized as an impl, not collapsed to a stray `impl` member — qual_path
#     must drop the impl's leading param list like group_key does.
repo=$(new_repo 'pub struct Foo;')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub struct Foo;\nimpl<\'a> From<&\'a u8> for Foo { fn from(_: &\'a u8) -> Self { Foo } }' 'add lifetime-param impl')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "impl From<&u8> for Foo" "--changelog: impl<'a> recognized; lifetime stripped"
case "$out" in
*'::impl`'* | *'- `impl`'*) bad "--changelog: impl<'a> must not collapse to a stray 'impl' member" ;;
*"'a"*) bad "--changelog: lifetime param should be stripped from the impl header" ;;
*) ok "--changelog: no stray 'impl' member / no lifetime from impl<'a>" ;;
esac
rm -rf "$repo"

# 6u) Generic args keep one module segment (not just the last), and nested
#     generics are extracted with balanced brackets: a local `sub::Bar` inside a
#     `core::option::Option<...>` renders as `From<option::Option<sub::Bar>>`.
repo=$(new_repo 'pub struct Foo;')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub struct Foo;\npub mod sub { pub struct Bar; }\nimpl From<core::option::Option<sub::Bar>> for Foo { fn from(_: core::option::Option<sub::Bar>) -> Self { Foo } }' 'add nested From')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "impl From<option::Option<sub::Bar>> for Foo" "--changelog: keep one module segment + nested generics"
rm -rf "$repo"

# 6r) Boilerplate trait methods implied by the impl (here `from`) are not listed
#     under the impl header.
repo=$(new_repo $'pub struct Foo;\npub struct Bar;')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub struct Foo;\npub struct Bar;\nimpl From<Bar> for Foo { fn from(_: Bar) -> Self { Foo } }' 'add From<Bar>')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "impl From<Bar> for Foo" "--changelog: From impl is documented"
case "$out" in
*'`from`'*) bad "--changelog: a From impl must not list its boilerplate 'from' method" ;;
*) ok "--changelog: boilerplate 'from' dropped under the impl header" ;;
esac
rm -rf "$repo"

# 6s) The compiler-internal `StructuralPartialEq` marker (emitted by a PartialEq
#     derive) is dropped, while the real `impl PartialEq` is kept.
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\n#[derive(PartialEq)]\npub struct X(pub u8);' 'derive PartialEq')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "impl PartialEq for X" "--changelog: real derived impl kept"
case "$out" in
*StructuralPartialEq*) bad "--changelog: StructuralPartialEq compiler marker must be dropped" ;;
*) ok "--changelog: StructuralPartialEq marker dropped" ;;
esac
rm -rf "$repo"

# 6k) A `const fn` must keep its name, not collapse to a stray `fn` group (the
#     keyword strip has to consume the `const`/`async`/`unsafe` qualifier *and*
#     the `fn`).
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub const fn answer() -> u8 { 42 }' 'add const fn')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`answer\`" "--changelog: const fn keeps its name"
case "$out" in
*'- `fn`'*) bad "--changelog: const fn must not collapse to a stray 'fn' group" ;;
*) ok "--changelog: no stray 'fn' group from const fn" ;;
esac
rm -rf "$repo"

# 6l) Auto-derived impls must surface (cargo-public-api runs at -ss, not -sss),
#     so a newly-derived `impl Hash` is documented rather than silently dropped.
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\n#[derive(Hash)]\npub struct K(pub u8);' 'derive Hash')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "impl Hash for K" "--changelog: -ss surfaces auto-derived impls"
rm -rf "$repo"

# 6t) Several member-less impls on one type collapse to `impl {A, B, ...} for T`
#     (lrz's many-traits-one-type form), e.g. a new type and its derives.
repo=$(new_repo 'pub struct Foo;')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub struct Foo;\n#[derive(Clone, Debug)]\npub struct Bar(pub u8);' 'derive Clone, Debug on Bar')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "impl {Clone, Debug} for Bar" "--changelog: derives on one type collapse to impl {..} for T"
rm -rf "$repo"

# 6w) A whole added module subsumes its contents: only the module is listed, not
#     the types/fns inside it (but sibling modules are kept).
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub fn sibling() -> u8 { 0 }\npub mod m { pub struct Foo; pub fn g() -> u8 { 0 } }' 'add module m and a sibling fn')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`m\`" "--changelog: an added module is listed on its own"
assert_contains "$out" "- \`sibling\`" "--changelog: items outside the module are unaffected"
case "$out" in
*Foo*|*'m::g'*) bad "--changelog: contents of an added module should be subsumed" ;;
*) ok "--changelog: added module subsumes its contents" ;;
esac
rm -rf "$repo"

echo ""
echo "passed: $pass  failed: $fail"
[ "$fail" -eq 0 ]
