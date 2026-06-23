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

# new_repo <lib.rs contents> -> prints the repo dir.
# Creates a single-crate cargo lib in a fresh git repo with a committed lockfile.
new_repo() {
    local d
    d=$(mktemp -d)
    git -C "$d" init -q
    git -C "$d" config user.email t@t
    git -C "$d" config user.name t
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
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 ); rc=$?
assert_eq "$rc" 1 "removed pub fn: exit 1"
assert_contains "$out" "BREAKING" "removed pub fn: BREAKING verdict"
assert_contains "$out" "foo" "removed pub fn: names the removed item"
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
json=$( cd "$repo" && "$ZIFF" --json "$base" "$head" 2>/dev/null )
if printf '%s' "$json" | jq -e 'has("totals") and has("crates") and .verdict == "breaking"' >/dev/null 2>&1; then
    ok "--json: valid shape, verdict=breaking"
else
    bad "--json: expected {totals, crates, verdict:breaking}, got: $(printf '%s' "$json" | head -c 120)"
fi
rm -rf "$repo"

# 5) --fetch with no remote fails fast with a clear error (no build needed).
repo=$(mktemp -d)
git -C "$repo" init -q
git -C "$repo" config user.email t@t
git -C "$repo" config user.name t
: >"$repo/x"
git -C "$repo" add -A
git -C "$repo" commit -qm init
out=$( cd "$repo" && "$ZIFF" --fetch 2>&1 ); rc=$?
assert_eq "$rc" 1 "--fetch with no remote: exit 1"
assert_contains "$out" "no remote found" "--fetch with no remote: clear error"
rm -rf "$repo"

# 6) Default grouping is by MODULE: items cluster under a module header with no
#    kind tag, and the shared module prefix is elided from each item.
repo=$(new_repo 'pub fn placeholder() {}')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub fn placeholder() {}\npub struct Widget;\nimpl Widget {\n    pub fn new() -> Self { Widget }\n    pub fn run(&self) {}\n}' 'add Widget type')
out=$( cd "$repo" && "$ZIFF" "$base" "$head" 2>&1 )
if printf '%s\n' "$out" | grep -qE '^ +ziff_fixture$'; then
    ok "mod grouping (default): bare module header, no kind tag"
else
    bad "mod grouping: expected a bare 'ziff_fixture' module header"
fi
assert_contains "$out" "+ pub struct Widget" "mod grouping: shared module prefix elided"

# 6b) --by-type groups by the owning type and tags the header with its kind.
byt=$( cd "$repo" && "$ZIFF" --by-type "$base" "$head" 2>&1 )
if printf '%s\n' "$byt" | grep -qE '^ +ziff_fixture::Widget +\(struct\)$'; then
    ok "--by-type: type header with (struct) tag"
else
    bad "--by-type: expected 'ziff_fixture::Widget  (struct)' header"
fi
# A pre-existing type (no declaration in the diff) still gets a fallback (type) tag.
head2=$(commit_lib "$repo" $'pub fn placeholder() {}\npub struct Widget;\nimpl Widget {\n    pub fn new() -> Self { Widget }\n    pub fn run(&self) {}\n    pub fn extra(&self) {}\n}' 'add a method to existing Widget')
byt2=$( cd "$repo" && "$ZIFF" --by-type "$head" "$head2" 2>&1 )
if printf '%s\n' "$byt2" | grep -qE '^ +ziff_fixture::Widget +\(type\)$'; then
    ok "--by-type: undeclared type gets fallback (type) tag"
else
    bad "--by-type: expected 'ziff_fixture::Widget  (type)' for undeclared type"
fi

# 6c) --flat keeps fully-qualified paths and emits no indented group header.
flat=$( cd "$repo" && "$ZIFF" --flat "$base" "$head" 2>&1 )
assert_contains "$flat" "pub struct ziff_fixture::Widget" "--flat: keeps fully-qualified paths"
# A group header is an indented path alone (optionally a kind tag); the per-crate
# summary line (which has counts after the name) must not be mistaken for one.
if printf '%s\n' "$flat" | grep -qE '^ +ziff_fixture(::[A-Za-z0-9_]+)*( +\([a-z]+\))?$'; then
    bad "--flat: should not emit a group header"
else
    ok "--flat: no group header"
fi
rm -rf "$repo"

echo ""
echo "passed: $pass  failed: $fail"
[ "$fail" -eq 0 ]
