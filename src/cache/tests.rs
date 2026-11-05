use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::sync::Mutex;

use super::{api_fingerprint, sha1_short, Cache, RunTmp};

#[test]
fn sha1_is_lowercase_and_truncated_to_twelve_hex_digits() {
    assert_eq!(sha1_short(b""), "da39a3ee5e6b");
    assert_eq!(sha1_short(b"abc"), "a9993e364706");
}

#[test]
fn api_fingerprint_tracks_flags_targets_and_compiler_configuration() {
    let empty = BTreeMap::new();
    let original = api_fingerprint("public-api 1", "rustc 1", &[], &empty);
    for name in [
        "RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
        "RUSTC",
        "RUSTDOC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        let mut environment = BTreeMap::new();
        environment.insert(OsString::from(name), OsString::from("--cfg\u{1f}api_a"));
        let first = api_fingerprint("public-api 1", "rustc 1", &[], &environment);
        environment.insert(OsString::from(name), OsString::from("--cfg\u{1f}api_b"));
        let second = api_fingerprint("public-api 1", "rustc 1", &[], &environment);
        assert_ne!(original, first, "{name}");
        assert_ne!(first, second, "{name}");
    }
    assert_ne!(
        original,
        api_fingerprint("public-api 2", "rustc 1", &[], &empty)
    );
    assert_ne!(
        original,
        api_fingerprint("public-api 1", "rustc 2", &[], &empty)
    );
    assert_ne!(
        original,
        api_fingerprint(
            "public-api 1",
            "rustc 1",
            &["--all-features".into()],
            &empty
        )
    );
}

#[test]
fn fingerprint_components_cannot_alias_through_embedded_newlines() {
    assert_ne!(
        api_fingerprint("tool", "compiler", &["a\nb".into()], &BTreeMap::new()),
        api_fingerprint(
            "tool",
            "compiler",
            &["a".into(), "b".into()],
            &BTreeMap::new()
        ),
    );
}

#[test]
fn external_cargo_configuration_invalidates_cache_without_pairing_ref_configs() {
    let tmp = RunTmp::new().unwrap();
    let baseline = tmp.dir.join("baseline");
    let head = tmp.dir.join("head");
    for worktree in [&baseline, &head] {
        fs::create_dir_all(worktree.join(".cargo")).unwrap();
    }
    let fingerprint = || {
        let mut cache = Cache {
            dir: tmp.dir.join("cache"),
            script_hash: String::new(),
            api_fp: "initial-fingerprint".into(),
            api_json: Mutex::new(BTreeMap::new()),
        };
        cache
            .configure_api("nightly-test", &[&baseline, &head])
            .unwrap();
        cache.api_fp
    };
    let original = fingerprint();
    fs::write(
        baseline.join(".cargo/config.toml"),
        "[build]\nrustflags=['--cfg=base']",
    )
    .unwrap();
    fs::write(
        head.join(".cargo/config.toml"),
        "[build]\nrustflags=['--cfg=head']",
    )
    .unwrap();
    assert_eq!(original, fingerprint());

    fs::create_dir_all(tmp.dir.join(".cargo")).unwrap();
    fs::write(
        tmp.dir.join(".cargo/config.toml"),
        "[build]\nrustdocflags=['--cfg=external']",
    )
    .unwrap();
    assert_ne!(original, fingerprint());
}
