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
assert_contains "$out" "- \`Widget\`:" "--changelog: type group header (crate-relative)"
assert_contains "$out" "- \`new\`" "--changelog: member shown as a bare name"
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

# 6j) --changelog attributes a trait-impl associated item to its `impl Trait for
#     Self` via the rustdoc trait map, recovering the Self generics from the
#     signature. Here the `Bytes` assoc type of `impl IntoDisk for Foo<u32>`
#     changes value, so it lands in ### Changed grouped under the impl header
#     rather than under a bare `Foo` type.
repo=$(new_repo $'pub trait IntoDisk { type Bytes; }\npub struct Foo<T>(pub T);\nimpl IntoDisk for Foo<u32> { type Bytes = [u8; 40]; }')
base=$(git -C "$repo" rev-parse HEAD)
head=$(commit_lib "$repo" $'pub trait IntoDisk { type Bytes; }\npub struct Foo<T>(pub T);\nimpl IntoDisk for Foo<u32> { type Bytes = [u8; 48]; }' 'bump Bytes')
out=$( cd "$repo" && "$ZIFF" --changelog "$base" "$head" 2>/dev/null )
assert_contains "$out" "- \`impl IntoDisk for Foo<u32>\`:" "--changelog: assoc item grouped under impl header with Self generics"
assert_contains "$out" "- \`Bytes\`" "--changelog: assoc type shown as a bare member under the impl"
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

echo ""
echo "passed: $pass  failed: $fail"
[ "$fail" -eq 0 ]
