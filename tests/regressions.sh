#!/usr/bin/env bash
# Sourced by run.sh; exercises API identity, compatibility and caller isolation.

regression_json() { # document, jq predicate, label
    if printf '%s' "$1" | jq -e "$2" >/dev/null 2>&1; then
        ok "$3"
    else
        bad "$3 (unexpected JSON: $1)"
    fi
}

regression_commit() { # repository, message
    (cd "$1" && cargo generate-lockfile --offline -q) || return
    git -C "$1" add -A && git -C "$1" commit -qm "$2" && git -C "$1" rev-parse HEAD
}

regression_package() { # directory, package name, library source
    mkdir -p "$1/src"
    printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n' "$2" >"$1/Cargo.toml"
    printf '%s\n' "$3" >"$1/src/lib.rs"
}

regression_output() { # action output file, key
    local key value
    while IFS='=' read -r key value; do
        if [ "$key" = "$2" ]; then printf '%s' "$value"; return; fi
    done <"$1"
}

regression_action_cache() { # action file, fixture directory
    [[ "$(uname -s)" == Linux ]] && getconf GNU_LIBC_VERSION >/dev/null 2>&1 || return 0
    local action_file=$1 root=$2 line body='' reading=false mode output path rc
    local cargo_home="$root/cache-cargo" rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"
    local cargo_path original_path=$PATH runner_os
    local -a root_override
    cargo_path=$(type -P cargo)
    mkdir -p "$cargo_home/bin" "$root/cache-home"
    ln -s "$cargo_path" "$cargo_home/bin/cargo"

    # Run the real eligibility body: unsafe installation layouts must not advertise a cache path.
    while IFS= read -r line || [ -n "$line" ]; do
        if [ "$line" = '    - name: Identify cargo-public-api cache' ]; then
            reading=header
        elif [ "$reading" = header ] && [ "$line" = '      run: |' ]; then
            reading=true
        elif [ "$reading" = true ]; then
            [[ -z "$line" || "$line" == '        '* ]] || break
            body+="${line#        }"$'\n'
        fi
    done <"$action_file"

    for mode in custom-home env-root empty-root config-root invalid-config fallback other-os; do
        output="$root/cache-$mode.output"
        rm -f "$cargo_home/config.toml" "$output"
        root_override=()
        path="$cargo_home/bin:$original_path"
        runner_os=Linux
        case "$mode" in
            env-root) root_override=("CARGO_INSTALL_ROOT=$root/install-root") ;;
            empty-root) root_override=('CARGO_INSTALL_ROOT=') ;;
            config-root) printf '[install]\nroot = "%s"\n' "$root/install-root" >"$cargo_home/config.toml" ;;
            invalid-config) printf 'invalid TOML\n' >"$cargo_home/config.toml" ;;
            fallback) path=$original_path ;;
            other-os) runner_os=Darwin ;;
        esac
        (cd "$root" && env -u CARGO_INSTALL_ROOT HOME="$root/cache-home" CARGO_HOME="$cargo_home" \
            RUSTUP_HOME="$rustup_home" PATH="$path" RUNNER_OS="$runner_os" GITHUB_OUTPUT="$output" \
            "${root_override[@]}" bash -c "$body")
        rc=$?
        path=''
        if [ -f "$output" ]; then path=$(regression_output "$output" path); fi
        if [ "$mode" = custom-home ]; then
            assert_eq "$rc:$path" "0:$cargo_home/bin/cargo-public-api" "action cache: custom Cargo home is the restore destination"
        else
            assert_eq "$rc:$path" '0:' "action cache: $mode retains installation without restoring a binary"
        fi
    done
}

audit_regressions() {
    local audit_root repo base head added_head json out rc member mode before after source consumer
    local oldreq newreq body line action_file reading report_first report_second notes_first
    local before_head before_status before_branch before_worktrees lock_before
    # Keep ambient build flags and shared targets out of otherwise independent fixtures.
    local -x TMPDIR CARGO_TARGET_DIR RUSTFLAGS RUSTDOCFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_ENCODED_RUSTDOCFLAGS
    unset CARGO_TARGET_DIR RUSTFLAGS RUSTDOCFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_ENCODED_RUSTDOCFLAGS
    audit_root=$(mktemp -d)
    TMPDIR=$audit_root
    export TMPDIR
    # Assertions never exit early; RETURN also cleans fixtures if a scenario returns early.
    trap 'rm -rf "$audit_root"' RETURN
    action_file="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/action.yml"

    echo "audited correctness regressions"
    regression_action_cache "$action_file" "$audit_root"

    # Analysis only removes its own temporary worktrees, not unrelated offline checkouts.
    repo=$(new_repo 'pub fn original() {}')
    git -C "$repo" worktree add --detach -q "$audit_root/offline-checkout" HEAD
    rm -rf "$audit_root/offline-checkout"
    git -C "$repo" config gc.worktreePruneExpire now
    printf 'pub fn replacement() {}\n' >"$repo/src/lib.rs"
    before=$(git -C "$repo" worktree list --porcelain)
    json=$(cd "$repo" && "$ZC" --json 2>/dev/null)
    assert_eq "$?" 1 "worktree isolation: API comparison completes"
    regression_json "$json" '.verdict == "breaking" and .totals.error_crates == 0' "worktree isolation: stale unrelated registration does not affect the verdict"
    after=$(git -C "$repo" worktree list --porcelain)
    assert_eq "$after" "$before" "worktree isolation: unrelated prunable registration survives"

    # Snapshots retain force-staged ignored files without borrowing a user commit identity.
    repo=$(new_repo 'pub fn original() {}')
    printf '/src/generated.rs\n' >>"$repo/.gitignore"
    printf 'pub fn replacement() {}\n' >"$repo/src/generated.rs"
    printf 'mod generated;\npub use generated::*;\n' >"$repo/src/lib.rs"
    git -C "$repo" add src/lib.rs
    git -C "$repo" add -f src/generated.rs
    before_head=$(git -C "$repo" rev-parse HEAD)
    before_status=$(git -C "$repo" status --porcelain)
    before_branch=$(git -C "$repo" branch --show-current)
    before_worktrees=$(worktree_count "$repo")
    json=$(cd "$repo" && "$ZC" --json 2>/dev/null)
    assert_eq "$?" 1 "dirty snapshot: force-staged ignored module is analyzed"
    regression_json "$json" '.verdict == "breaking" and .totals.error_crates == 0 and .totals.removed == 1 and .totals.added == 1' "dirty snapshot: ignored source retains the actual public change"
    assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "dirty snapshot: staged content"

    git -C "$repo" config --unset user.name
    git -C "$repo" config --unset user.email
    git -C "$repo" config user.useConfigOnly true
    json=$(cd "$repo" && env -u GIT_AUTHOR_NAME -u GIT_AUTHOR_EMAIL -u GIT_COMMITTER_NAME -u GIT_COMMITTER_EMAIL -u EMAIL GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 "$ZC" --json 2>/dev/null)
    assert_eq "$?" 1 "dirty snapshot: no configured Git identity is required"
    regression_json "$json" '.verdict == "breaking" and .totals.error_crates == 0 and .totals.removed == 1 and .totals.added == 1' "dirty snapshot: identity-free comparison retains the API diff"

    # Copied performance flags and stat caches must not replace physical source with staged content.
    repo=$(new_repo 'pub fn stable() {}')
    printf 'pub fn staged() {}\n' >"$repo/src/lib.rs"
    git -C "$repo" add src/lib.rs
    git -C "$repo" update-index --assume-unchanged src/lib.rs
    printf 'pub fn stable() {}\n' >"$repo/src/lib.rs"
    before_head=$(git -C "$repo" rev-parse HEAD)
    before_status=$(git -C "$repo" status --porcelain)
    before_branch=$(git -C "$repo" branch --show-current)
    before_worktrees=$(worktree_count "$repo")
    before=$(git -C "$repo" ls-files --stage && git -C "$repo" ls-files -v)
    json=$(cd "$repo" && "$ZC" --json 2>/dev/null)
    assert_eq "$?" 0 "dirty snapshot: physical source takes precedence over staged contents"
    regression_json "$json" '.verdict == "ok" and .totals.error_crates == 0 and .totals.api_breaking == 0 and .totals.added == 0' "dirty snapshot: index assumptions do not invent an API change"
    after=$(git -C "$repo" ls-files --stage && git -C "$repo" ls-files -v)
    assert_eq "$after" "$before" "dirty snapshot: real index entries and flags stay unchanged"
    assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "dirty snapshot: assumed-unchanged source"

    # Manual skip bits cannot hide source; real sparse checkouts retain unmaterialized files.
    for mode in skip-worktree sparse; do
        repo=$(new_repo 'pub fn stable() {}')
        if [ "$mode" = sparse ]; then
            mkdir -p "$repo/notes"
            printf 'tracked sparse data\n' >"$repo/notes/kept.txt"
            git -C "$repo" add notes
            git -C "$repo" commit -qm 'tracked data outside sparse selection'
            git -C "$repo" sparse-checkout set --no-cone /src/ /Cargo.toml /Cargo.lock /.gitignore
        else
            git -C "$repo" config core.sparseCheckout false
            git -C "$repo" update-index --skip-worktree src/lib.rs
            printf 'snapshot trigger\n' >"$repo/trigger.txt"
        fi
        printf 'pub fn replacement() {}\n' >"$repo/src/lib.rs"
        before_head=$(git -C "$repo" rev-parse HEAD)
        before_status=$(git -C "$repo" status --porcelain)
        before_branch=$(git -C "$repo" branch --show-current)
        before_worktrees=$(worktree_count "$repo")
        before=$(git -C "$repo" ls-files --stage && git -C "$repo" ls-files -v)
        json=$(cd "$repo" && "$ZC" --json 2>/dev/null)
        assert_eq "$?" 1 "index mode $mode: physical API removal is detected"
        regression_json "$json" '.verdict == "breaking" and .totals.error_crates == 0 and .totals.removed > 0' "index mode $mode: no skipped-source false clean or sparse build error"
        after=$(git -C "$repo" ls-files --stage && git -C "$repo" ls-files -v)
        assert_eq "$after" "$before" "index mode $mode: real index content and flags are preserved"
        assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "index mode $mode: caller isolation"
    done

    # Running below the repository root must not dirty source directories with cache files.
    repo=$(new_repo 'pub fn original() {}')
    printf 'pub fn replacement() {}\n' >"$repo/src/lib.rs"
    before_head=$(git -C "$repo" rev-parse HEAD)
    before_status=$(git -C "$repo" status --porcelain)
    before_branch=$(git -C "$repo" branch --show-current)
    before_worktrees=$(worktree_count "$repo")
    json=$(cd "$repo/src" && "$ZC" --json 2>/dev/null)
    assert_eq "$?" 1 "nested invocation: dirty API comparison succeeds"
    regression_json "$json" '.verdict == "breaking" and .totals.error_crates == 0 and .totals.removed == 1 and .totals.added == 1' "nested invocation: compares the full workspace"
    assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "nested invocation: caller isolation"

    # Ref-local Cargo configuration and the actual [lib] target name both matter.
    repo=$(new_repo $'pub fn stable() {}\n#[cfg(custom_api)]\npub fn configured() {}')
    printf '\n[lib]\nname = "public_name"\n' >>"$repo/Cargo.toml"
    mkdir -p "$repo/.cargo"
    printf '[build]\nrustdocflags = []\n' >"$repo/.cargo/config.toml"
    base=$(regression_commit "$repo" 'custom library without cfg')
    printf '[build]\nrustdocflags = ["--cfg", "custom_api"]\n' >"$repo/.cargo/config.toml"
    head=$(regression_commit "$repo" 'enable ref-local cfg')
    json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
    rc=$?
    assert_eq "$rc" 0 "ref config/custom library: analysis succeeds"
    regression_json "$json" '.totals.error_crates == 0 and (.crates | any(.name == "zc_fixture" and .lib_name == "public_name" and .added > 0 and .removed == 0))' "ref config/custom library: cfg API is added only at head"
    # The reverse direction must not inherit the caller checkout's configuration either.
    json=$(cd "$repo" && "$ZC" --json "$head" "$base" 2>/dev/null)
    assert_eq "$?" 1 "ref config: reversed comparison detects the removed cfg API"
    regression_json "$json" '.totals.api_breaking > 0 and .totals.error_crates == 0' "ref config: reverse comparison is a real API break"

    # Same refs and target directory, different rustdoc flags: warm and fresh agree.
    repo=$(new_repo $'#[cfg(has_api)]\npub fn removed() {}')
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" '' 'remove cfg API')
    json=$(cd "$repo" && CARGO_TARGET_DIR="$repo/target/shared" "$ZC" --json "$base" "$head" 2>/dev/null)
    regression_json "$json" '.verdict == "ok" and .totals.api_breaking == 0' "flags cache: unconfigured API is absent"
    for mode in shared fresh; do
        json=$(cd "$repo" && CARGO_TARGET_DIR="$repo/target/$mode" RUSTDOCFLAGS='--cfg=has_api' "$ZC" --json "$base" "$head" 2>/dev/null)
        assert_eq "$?" 1 "flags cache: $mode flags detect removal"
        regression_json "$json" '.totals.error_crates == 0 and .totals.api_breaking > 0' "flags cache: $mode result reflects rustdoc flags"
    done

    # Cargo normalizes both package names to foo_bar; snapshots must not alias.
    repo=$(new_repo 'pub fn root() {}')
    printf '\n[workspace]\nmembers = ["hyphen", "underscore"]\nresolver = "2"\n' >>"$repo/Cargo.toml"
    regression_package "$repo/hyphen" foo-bar 'pub fn removed() {}'
    regression_package "$repo/underscore" foo_bar 'pub fn unchanged() {}'
    base=$(regression_commit "$repo" 'colliding library names')
    printf '\n' >"$repo/hyphen/src/lib.rs"
    head=$(regression_commit "$repo" 'remove only hyphen API')
    for mode in cold warm; do
        json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
        assert_eq "$?" 1 "artifact collision: $mode comparison detects removal"
        regression_json "$json" '.totals.error_crates == 0 and (.crates | any(.name == "foo-bar" and .removed > 0)) and (.crates | any(.name == "foo_bar" and .removed == 0 and .changed == 0 and .added == 0))' "artifact collision: $mode results stay package-specific"
    done

    # Workspace membership is compared as a union. Unpublished path libraries remain API.
    repo=$(new_repo 'pub fn root() {}')
    printf '\n[workspace]\nmembers = ["gone", "private", "tool"]\nresolver = "2"\n' >>"$repo/Cargo.toml"
    regression_package "$repo/gone" gone 'pub fn removed() {}'
    regression_package "$repo/private" private_lib 'pub fn old_api() {}'
    printf 'publish = false\n' >>"$repo/private/Cargo.toml"
    regression_package "$repo/tool" tool 'pub fn unused() {}'
    rm "$repo/tool/src/lib.rs"
    printf 'fn main() {}\n' >"$repo/tool/src/main.rs"
    base=$(regression_commit "$repo" 'workspace baseline')
    # First compare a published crate addition on its own.
    printf '[package]\nname = "zc_fixture"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\nmembers = ["gone", "born", "private", "tool"]\nresolver = "2"\n' >"$repo/Cargo.toml"
    regression_package "$repo/born" born 'pub fn added() {}'
    added_head=$(regression_commit "$repo" 'add published crate')
    json=$(cd "$repo" && "$ZC" --json "$base" "$added_head" 2>/dev/null)
    assert_eq "$?" 0 "crate membership: a new published library is additive"
    regression_json "$json" '.verdict == "ok" and .totals.error_crates == 0 and (.crates | any(.name == "born" and .change == "added" and .added > 0))' "crate membership: new library API is reported, not skipped"
    printf '[package]\nname = "zc_fixture"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\nmembers = ["born", "private", "tool"]\nresolver = "2"\n' >"$repo/Cargo.toml"
    rm -rf "$repo/gone"
    printf 'pub fn new_api() {}\n' >"$repo/private/src/lib.rs"
    head=$(regression_commit "$repo" 'replace member and private API')
    json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
    assert_eq "$?" 1 "crate membership: removed and unpublished APIs fail"
    regression_json "$json" '.totals.error_crates == 0 and (.crates | any(.name == "gone" and .change == "removed" and .removed > 0)) and (.crates | any(.name == "born" and .change == "added" and .added > 0)) and (.crates | any(.name == "private_lib" and .removed > 0)) and (.skipped_crates | index("tool") != null) and (.crates | all(.name != "tool"))' "crate membership: additions/removals, private libraries and binary skip are explicit"

    # A failed one-sided build must still identify the crate's lifecycle in human output.
    printf 'pub fn invalid(\n' >"$repo/born/src/lib.rs"
    head=$(regression_commit "$repo" 'invalid new library')
    out=$(cd "$repo" && NO_COLOR=1 "$ZC" "$base" "$head" 2>&1)
    assert_eq "$?" 2 "crate membership: an invalid added library is an analysis error"
    assert_contains "$out" 'born (crate added)' "crate membership: added build errors retain lifecycle context"
    out=$(cd "$repo" && NO_COLOR=1 "$ZC" "$head" "$base" 2>&1)
    assert_eq "$?" 2 "crate membership: an invalid removed library is an analysis error"
    assert_contains "$out" 'born (crate removed)' "crate membership: removed build errors retain lifecycle context"

    # All renderer assertions name actual public API; no bullet order/wording contract.
    before=$'pub type Strategy = u8;\npub struct S;\nimpl Default for S { fn default() -> Self { S } }\npub struct Attr { pub x: u8 }'
    after=$'pub type Strategy = u16;\npub struct S;\nimpl Default for S { fn default() -> Self { S } }\nimpl S { pub fn default(_: usize) -> Self { S } }\nimpl From<u8> for S { fn from(_: u8) -> Self { S } }\nimpl From<u16> for S { fn from(_: u16) -> Self { S } }\n#[non_exhaustive] pub struct Attr { pub x: u8 }\n#[macro_export] macro_rules! one { () => {} }\n#[macro_export] macro_rules! two { () => {} }\n#[no_mangle] #[link_section=".custom"] pub static COUNT: u8 = 0;\npub extern "C" fn exported() {}'
    repo=$(new_repo "$before")
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" "$after" 'change aliases and add distinct public items')
    out=$(cd "$repo" && "$ZC" --changelog --fail-on api-breaking --report "$audit_root/render.json" "$base" "$head" 2>/dev/null)
    assert_eq "$?" 1 "changelog: explicit API-breaking policy is honored"
    for member in Strategy 'From<u8>' 'From<u16>' default non_exhaustive one two COUNT exported; do
        assert_contains "$out" "$member" "changelog identity: retains $member"
    done
    case "$out" in
        *'impl Default'*) bad "changelog attribution: unchanged Default impl was attributed an inherent method" ;;
        *) ok "changelog attribution: inherent default is not an unchanged trait impl" ;;
    esac
    out=$(cd "$repo" && "$ZC" --changelog --fail-on none "$base" "$head" 2>/dev/null)
    assert_eq "$?" 0 "changelog: fail-on none permits a breaking draft"
    regression_json "$(cat "$audit_root/render.json")" '.verdict == "breaking" and .totals.api_breaking > 0' "changelog: policy does not erase the semantic verdict"

    # A local module named bar is not evidence about the optional external dependency.
    repo=$(new_pubdep_repo 'bar = { path = "../bar", version = "0.1", optional = true }' 'pub mod bar { pub fn f() {} }')
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(bump_pubdep_head "$repo" 'bar = { path = "../bar", version = "0.2", optional = true }')
    json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
    assert_eq "$?" 0 "public deps: local module does not make an optional dep public"
    regression_json "$json" '.totals.public_dep_breaking == 0 and (.public_dep_breaks | length == 0)' "public deps: local paths do not masquerade as foreign types"

    # Reexports retain foreign provenance, including a non-package library target name.
    repo=$(new_pubdep_repo 'bar = { path = "../bar", version = "0.1", optional = true }' '#[cfg(feature = "bar")] pub use foreign_api::Error;')
    printf '\n[lib]\nname = "foreign_api"\n' >>"$repo/bar/Cargo.toml"
    base=$(regression_commit "$repo" 'custom dependency lib reexport')
    head=$(bump_pubdep_head "$repo" 'bar = { path = "../bar", version = "0.2", optional = true }')
    json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
    assert_eq "$?" 1 "public deps: reexported custom-lib dependency break fails"
    regression_json "$json" '.totals.error_crates == 0 and (.public_dep_breaks | any(.crate == "foo" and .dep == "bar" and .class == "breaking"))' "public deps: reexport preserves custom-library provenance"

    # Ceiling changes and prerelease comparators must not collapse into major bumps.
    for mode in ceiling prerelease; do
        if [ "$mode" = ceiling ]; then
            oldreq='<0.2'; newreq='<0.3'
        else
            oldreq='>=0.1.0-alpha, <0.2'; newreq='>=0.1.0-alpha, <0.3'
        fi
        repo=$(new_pubdep_repo "bar = { path = \"../bar\", version = \"$oldreq\", optional = true }" '#[cfg(feature = "bar")] pub fn f() -> bar::Error { bar::Error }')
        base=$(git -C "$repo" rev-parse HEAD)
        printf '[package]\nname = "foo"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\nbar = { path = "../bar", version = "%s", optional = true }\n' "$newreq" >"$repo/foo/Cargo.toml"
        head=$(regression_commit "$repo" 'widen upper requirement bound')
        json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
        assert_eq "$?" 0 "requirements: widened $mode does not fail"
        regression_json "$json" '.verdict == "ok" and .totals.public_dep_breaking == 0 and (.public_dep_breaks | any(.crate == "foo" and .dep == "bar" and .class == "review"))' "requirements: $mode is review, not a fabricated incompatibility"
    done

    # Target rows cannot be deduplicated by only package/version/kind.
    repo=$(new_pubdep_repo 'bar = { path = "../bar", version = ">=0.1,<0.5", optional = true }' 'pub fn stable() {}')
    printf '\n[target.\x27cfg(windows)\x27.dependencies]\nbar = { path = "../bar", version = ">=0.1,<0.2", optional = true }\n' >>"$repo/foo/Cargo.toml"
    base=$(regression_commit "$repo" 'target-specific dependency baseline')
    printf '[package]\nname = "foo"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\nbar = { path = "../bar", version = ">=0.1,<0.5", optional = true }\n[target.\x27cfg(windows)\x27.dependencies]\nbar = { path = "../bar", version = ">=0.1,<0.3", optional = true }\n' >"$repo/foo/Cargo.toml"
    head=$(regression_commit "$repo" 'widen only windows dependency')
    json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
    regression_json "$json" '.totals.error_crates == 0 and (.deps.changed | any(.name == "bar" and (.old | contains("<0.2")) and (.new | contains("<0.3"))))' "target dependency: changed requirement survives workspace aggregation"
    out=$(cd "$repo" && "$ZC" --changelog "$base" "$head" 2>/dev/null)
    assert_contains "$out" 'cfg(windows)' "target dependency: changelog preserves platform context"

    # Strong optional slash enables the local dep feature; required/weak slash does not.
    repo=$(new_repo $'#[cfg(feature = "public")] pub fn required_slash() {}\n#[cfg(feature = "weak")] pub fn weak_slash() {}\n#[cfg(feature = "strong")] pub fn strong_slash() {}')
    cat >>"$repo/Cargo.toml" <<'EOF'
[workspace]
members = ["helper", "optional"]
resolver = "2"
[dependencies]
helper = { path = "helper" }
optional = { path = "optional", optional = true }
[features]
helper = ["fuzzing"]
optional = ["dep:optional", "fuzzing"]
fuzzing = []
public = ["helper/std"]
weak = ["optional?/std"]
strong = ["optional/std"]
EOF
    for member in helper optional; do
        regression_package "$repo/$member" "$member" 'pub fn stable() {}'
        printf '\n[features]\nstd = []\n' >>"$repo/$member/Cargo.toml"
    done
    base=$(regression_commit "$repo" 'slash feature graph')
    head=$(commit_lib "$repo" '' 'remove slash gated APIs')
    out=$(cd "$repo" && "$ZC" --flat --features exclude:fuzzing --report "$audit_root/slash.json" "$base" "$head" 2>/dev/null)
    assert_eq "$?" 1 "feature slash: kept features expose an API removal"
    assert_contains "$out" required_slash "feature slash: required dep does not enable same-named local feature"
    assert_contains "$out" weak_slash "feature slash: weak optional dep does not enable local feature"
    case "$out" in
        *strong_slash*) bad "feature slash: strong optional dep escaped the excluded local feature" ;;
        *) ok "feature slash: strong optional activation propagates exclusion" ;;
    esac
    regression_json "$(cat "$audit_root/slash.json")" '.totals.error_crates == 0 and .totals.api_breaking > 0' "feature slash: valid build, not a feature-resolution error"

    # Values and documentation traverse public associated/implicit-public members.
    before=$'pub struct S; impl S { pub const VALUE: u32 = 1; }\npub fn first_doc() {}\n/// old\npub fn last_doc() {}\npub enum E { /// old\n V }\npub trait T { /// old\n fn f(); }'
    after=$'pub struct S; impl S { pub const VALUE: u32 = 2; }\n/// new\npub fn first_doc() {}\npub fn last_doc() {}\npub enum E { /// new\n V }\npub trait T { /// new\n fn f(); }'
    repo=$(new_repo "$before")
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" "$after" 'associated values and public docs')
    json=$(cd "$repo" && "$ZC" --json --with-values "$base" "$head" 2>/dev/null)
    assert_eq "$?" 1 "values/docs: changed constant is breaking"
    regression_json "$json" '.verdict == "breaking" and .totals.error_crates == 0 and (.values | any((.path | endswith("::S::VALUE")) and .old != .new and .old != "?" and .new != "?"))' "values: associated const initializer is compared"
    for member in first_doc last_doc E::V T::f; do
        regression_json "$json" ".docs | any(.path | endswith(\"::$member\"))" "docs: detects $member documentation change"
    done

    # --with-lock must use compared refs, not an unrelated dirty caller manifest.
    repo=$(new_repo 'pub fn stable() {}')
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" $'pub fn stable() {}\npub fn added() {}' 'head API')
    regression_package "$audit_root/dirty_dep" dirty_dep ''
    printf '\n[dependencies]\ndirty_dep = { path = "%s/dirty_dep" }\n' "$audit_root" >>"$repo/Cargo.toml"
    lock_before=$(cat "$repo/Cargo.lock")
    before_head=$(git -C "$repo" rev-parse HEAD)
    before_status=$(git -C "$repo" status --porcelain)
    before_branch=$(git -C "$repo" branch --show-current)
    before_worktrees=$(worktree_count "$repo")
    json=$(cd "$repo" && "$ZC" --json --with-lock "$base" "$head" 2>/dev/null)
    assert_eq "$?" 0 "lock isolation: dirty caller dependency cannot fail ref analysis"
    regression_json "$json" '.verdict == "ok" and .totals.error_crates == 0' "lock isolation: compared refs produce a trustworthy result"
    assert_eq "$(cat "$repo/Cargo.lock")" "$lock_before" "lock isolation: caller lockfile bytes preserved"
    assert_repo_unchanged "$repo" "$before_head" "$before_status" "$before_branch" "$before_worktrees" "lock isolation"

    # Additive textual diffs can still invalidate real downstream Rust programs.
    before=$'pub enum State { One }\npub struct Record { pub x: u8 }\npub trait Required { fn old(&self); }'
    after=$'pub enum State { One, Two }\npub struct Record { pub x: u8, pub y: u8 }\npub trait Required { fn old(&self); fn added(&self); }'
    repo=$(new_repo "$before")
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" "$after" 'breaking additions to existing owners')
    json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
    assert_eq "$?" 1 "breaking additions: enum, struct and required trait members fail"
    regression_json "$json" '.totals.api_breaking > 0 and .totals.error_crates == 0' "breaking additions: affect the API verdict without build errors"
    for member in State Record Required; do
        regression_json "$json" ".crates | any(.breaking_added | any(contains(\"$member\")))" "breaking additions: identifies $member"
    done
    consumer="$audit_root/consumer"
    regression_package "$consumer" consumer ''
    printf '\n[dependencies]\nzc_fixture = { path = "%s" }\n' "$repo" >>"$consumer/Cargo.toml"
    for member in enum struct trait; do
        case "$member" in
            enum) source='pub fn use_api(x: zc_fixture::State) { match x { zc_fixture::State::One => () } }' ;;
            struct) source='pub fn use_api() -> zc_fixture::Record { zc_fixture::Record { x: 0 } }' ;;
            trait) source='pub struct Consumer; impl zc_fixture::Required for Consumer { fn old(&self) {} }' ;;
        esac
        printf '%s\n' "$source" >"$consumer/src/lib.rs"
        git -C "$repo" checkout -q "$base"
        (cd "$consumer" && cargo check --offline -q) >"$audit_root/consumer.log" 2>&1
        assert_eq "$?" 0 "downstream $member: baseline program compiles"
        git -C "$repo" checkout -q "$head"
        if (cd "$consumer" && cargo check --offline -q) >"$audit_root/consumer.log" 2>&1; then
            bad "downstream $member: breaking addition still compiled"
        else
            ok "downstream $member: head rejects the formerly valid program"
        fi
    done

    # Boundaries: non_exhaustive/private owners and provided/sealed trait methods.
    before=$'#[non_exhaustive] pub enum Open { One }\n#[non_exhaustive] pub struct OpenRecord { pub x: u8 }\npub struct PrivateRecord { pub x: u8, hidden: u8 }\npub trait Provided {}\nmod sealed { pub trait Sealed {} }\npub trait SealedApi: sealed::Sealed {}'
    after=$'#[non_exhaustive] pub enum Open { One, Two }\n#[non_exhaustive] pub struct OpenRecord { pub x: u8, pub y: u8 }\npub struct PrivateRecord { pub x: u8, hidden: u8, pub y: u8 }\npub trait Provided { fn added(&self) {} }\nmod sealed { pub trait Sealed {} }\npub trait SealedApi: sealed::Sealed { fn added(&self); }\npub enum NewEnum { One, Two }\npub struct NewRecord { pub x: u8, pub y: u8 }\npub trait NewTrait { fn required(&self); }'
    repo=$(new_repo "$before")
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" "$after" 'nonbreaking addition boundaries')
    json=$(cd "$repo" && "$ZC" --json "$base" "$head" 2>/dev/null)
    assert_eq "$?" 0 "addition boundaries: nonbreaking owners and methods pass"
    regression_json "$json" '.verdict == "ok" and .totals.error_crates == 0 and .totals.api_breaking == 0 and (.crates | all(.breaking_added | length == 0))' "addition boundaries: no breaking-addition false positives"

    # A lost struct literal can break callers without changing textual API signatures.
    repo=$(new_repo 'pub struct S { pub x: u8 }')
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" 'pub struct S { pub x: u8, hidden: u8 }' 'remove literal construction')
    out=$(cd "$repo" && "$ZC" "$base" "$head" 2>&1)
    assert_eq "$?" 1 "semantic-only change: the human report fails"
    assert_contains "$out" 'zc_fixture::S' "semantic-only change: the human report identifies the affected type"
    out=$(cd "$repo" && "$ZC" --changelog --report "$audit_root/semantic.json" "$base" "$head" 2>/dev/null)
    assert_eq "$?" 1 "semantic-only change: drafting retains the breaking policy"
    assert_contains "$out" '`S`' "semantic-only change: the draft identifies the affected type"
    regression_json "$(cat "$audit_root/semantic.json")" '.verdict == "breaking" and .totals.error_crates == 0 and (.crates | any(.breaking_added | any(startswith("zc_fixture::S:"))))' "semantic-only change: JSON retains its compatibility finding"

    # Execute the action's real final run body, not a source-text/wiring assertion.
    body=''; reading=false
    while IFS= read -r line || [ -n "$line" ]; do
        if [ "$line" = '      run: |' ]; then
            body=''; reading=true
        elif [ "$reading" = true ]; then
            body+="${line#        }"$'\n'
        fi
    done <"$action_file"
    repo=$(new_repo 'pub fn removed() {}')
    base=$(git -C "$repo" rev-parse HEAD)
    head=$(commit_lib "$repo" 'pub fn added() {}' 'action break')
    mkdir -p "$audit_root/runner/zc-bin/bin"
    ln -s "$ZC" "$audit_root/runner/zc-bin/bin/zc"
    (cd "$repo" && RUNNER_TEMP="$audit_root/runner" INPUT_BASELINE='--version' INPUT_HEAD='' INPUT_WITH_LOCK=false INPUT_WITH_VALUES=false INPUT_WITH_CHANGELOG=true INPUT_FAIL_ON=none INPUT_FEATURES=all GITHUB_OUTPUT="$audit_root/invalid.output" bash -c "$body") >"$audit_root/action.log" 2>&1
    rc=$?
    if [ "$rc" -ne 0 ]; then ok "action: option-like baseline is rejected"; else bad "action: baseline was parsed as a CLI option"; fi
    (cd "$repo" && RUNNER_TEMP="$audit_root/runner" INPUT_BASELINE="$base" INPUT_HEAD="$head" INPUT_WITH_LOCK=false INPUT_WITH_VALUES=false INPUT_WITH_CHANGELOG=true INPUT_FAIL_ON=none INPUT_FEATURES=all GITHUB_OUTPUT="$audit_root/first.output" bash -c "$body") >"$audit_root/action.log" 2>&1
    assert_eq "$?" 0 "action: fail-on none retains a breaking report"
    report_first=$(regression_output "$audit_root/first.output" report)
    notes_first=$(regression_output "$audit_root/first.output" changelog)
    if [ -f "$report_first" ] && [ -f "$notes_first" ]; then
        regression_json "$(cat "$report_first")" '.verdict == "breaking"' "action: first report records the API break"
        cp "$notes_first" "$audit_root/first-notes.saved"
    else
        bad "action: first invocation did not produce report and changelog files"
    fi
    (cd "$repo" && RUNNER_TEMP="$audit_root/runner" INPUT_BASELINE="$head" INPUT_HEAD="$head" INPUT_WITH_LOCK=false INPUT_WITH_VALUES=false INPUT_WITH_CHANGELOG=true INPUT_FAIL_ON=none INPUT_FEATURES=all GITHUB_OUTPUT="$audit_root/second.output" bash -c "$body") >"$audit_root/action.log" 2>&1
    assert_eq "$?" 0 "action: repeated invocation succeeds"
    report_second=$(regression_output "$audit_root/second.output" report)
    if [ -f "$report_first" ] && [ -f "$report_second" ]; then
        regression_json "$(cat "$report_first")" '.verdict == "breaking"' "action: later invocation preserves the first report"
        regression_json "$(cat "$report_second")" '.verdict == "ok"' "action: second invocation has an independent verdict"
    else
        bad "action: repeated invocation lost a report"
    fi
    if [ -f "$audit_root/first-notes.saved" ] && cmp -s "$notes_first" "$audit_root/first-notes.saved"; then
        ok "action: repeated invocation preserves the first changelog"
    else
        bad "action: repeated invocation overwrote the first changelog"
    fi

    rm -rf "$audit_root"
    trap - RETURN
}

audit_regressions
