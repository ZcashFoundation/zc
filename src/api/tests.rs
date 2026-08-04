use super::{hint, parse_diff};

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn parses_all_diff_sections_and_interleaves_changed_pairs() {
    let output = "\
-outside section
+outside section

Removed items from the public API
=============================
-old::gone()
(none)

Changed items in the public API
=============================
-old::first()
+new::first()
= unchanged::item()
-old::second()

Added items to the public API
=============================
+new::added()
(none)

+new::also_added()
";

    let parsed = parse_diff(output);

    assert_eq!(parsed.removed, strings(&["old::gone()"]));
    assert_eq!(
        parsed.changed,
        strings(&[
            "  - old::first()",
            "  + new::first()",
            "  - old::second()",
            "  + ",
        ])
    );
    assert_eq!(
        parsed.added,
        strings(&["new::added()", "new::also_added()"])
    );
    assert_eq!(parsed.removed.len(), 1);
    assert_eq!(parsed.changed_old_count, 2);
    assert_eq!(parsed.added.len(), 2);
}

#[test]
fn selects_hints_case_insensitively_in_spec_order() {
    const INSTALL_PROTOC: &str = concat!(
        "Install protoc, for example brew install protobuf or apt-get install ",
        "protobuf-compiler, then rerun zc."
    );
    const BUILD_SCRIPT: &str = concat!(
        "A build script failed. Run the command shown above to inspect the crate's build ",
        "requirements."
    );
    const LOCKFILE: &str = concat!(
        "The lockfile or dependency resolution failed at this ref. Check Cargo.lock and ",
        "rerun zc."
    );
    const TOOLCHAIN: &str = concat!(
        "The selected Rust toolchain cannot build this ref. Install the required toolchain ",
        "and rerun zc."
    );
    const LIBRARY: &str = concat!(
        "cargo-public-api can only analyze library targets. Exclude this crate or add a ",
        "library target."
    );
    const COMPILE: &str = concat!(
        "The crate did not compile under the selected feature set. Fix the build or choose a ",
        "supported feature policy."
    );
    const FALLBACK: &str = concat!(
        "Run the command shown above and fix the failing crate build before trusting the API ",
        "diff."
    );

    let cases = [
        ("PROTOC was not found", INSTALL_PROTOC),
        ("install protobuf-compiler", INSTALL_PROTOC),
        ("protoc: custom build command failed", INSTALL_PROTOC),
        ("failed to run custom build command", BUILD_SCRIPT),
        ("Cargo.lock needs to be updated", LOCKFILE),
        ("failed to read lock file", LOCKFILE),
        ("dependency lockfile is invalid", LOCKFILE),
        ("package requires rustc 1.90", TOOLCHAIN),
        ("rustc 1.70 is not supported", TOOLCHAIN),
        ("package has no library targets", LIBRARY),
        ("package does not have a library target", LIBRARY),
        ("could not compile example", COMPILE),
        ("an unrelated failure", FALLBACK),
    ];

    for (stderr, expected) in cases {
        assert_eq!(hint(stderr), expected, "stderr: {stderr}");
    }
}
