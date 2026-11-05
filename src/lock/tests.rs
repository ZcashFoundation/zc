use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{attribution, build_diff, diff_worktrees, extract_lock};

#[test]
fn parser_emits_a_trailing_package_without_a_blank_line() {
    let lock = concat!(
        "version = 3\n\n",
        "[[package]]\n",
        "name = \"alpha\"\n",
        "version = \"1.0.0\"\n\n",
        "[[package]]\n",
        "name = \"omega\"\n",
        "version = \"9.0.0\"",
    );

    assert_eq!(
        extract_lock(lock),
        vec![
            ("alpha".to_string(), "1.0.0".to_string()),
            ("omega".to_string(), "9.0.0".to_string()),
        ]
    );
}

#[test]
fn attribution_deduplicates_in_first_seen_order_and_truncates() {
    let direct = HashSet::from([
        "first".to_string(),
        "second".to_string(),
        "third".to_string(),
        "fourth".to_string(),
    ]);
    let tree = concat!(
        "0root-a v1\n",
        "1first v1\n",
        "2target v1\n",
        "0root-b v1\n",
        "1second v1\n",
        "2target v1\n",
        "0root-c v1\n",
        "1first v1\n",
        "2target v1\n",
        "1third v1\n",
        "2target v1\n",
        "0root-d v1\n",
        "1fourth v1\n",
        "2target v1\n",
    );

    let via = attribution(tree, &direct);

    assert_eq!(via["target"], "first,second,third,...(+1)");
}

#[test]
fn attribution_restores_ancestors_and_exits_sibling_subtrees() {
    let direct = HashSet::from(["outer".to_string(), "inner".to_string()]);
    let tree = concat!(
        "0root v1\n",
        "1outer v1\n",
        "2inner v1\n",
        "3inside v1\n",
        "2sibling v1\n",
        "3descendant v1\n",
        "1unrelated v1\n",
        "2outside v1\n",
        "0other-root v1\n",
        "1elsewhere v1\n",
    );

    let via = attribution(tree, &direct);

    assert_eq!(via["inside"], "inner");
    assert_eq!(via["sibling"], "outer");
    assert_eq!(via["descendant"], "outer");
    for name in ["unrelated", "outside", "elsewhere"] {
        assert!(!via.contains_key(name), "unexpected attribution for {name}");
    }
}

#[test]
fn lock_diff_groups_versions_and_suppresses_direct_dependencies() {
    let base = vec![
        ("direct".to_string(), "1.0.0".to_string()),
        ("multi".to_string(), "1.0.0".to_string()),
        ("multi".to_string(), "2.0.0".to_string()),
        ("removed".to_string(), "1.0.0".to_string()),
    ];
    let head = vec![
        ("added".to_string(), "1.0.0".to_string()),
        ("direct".to_string(), "2.0.0".to_string()),
        ("multi".to_string(), "2.0.0".to_string()),
        ("multi".to_string(), "3.0.0".to_string()),
    ];
    let direct = HashSet::from(["direct".to_string()]);

    let result = build_diff(&base, &head, &direct, "");

    assert_eq!(
        result.changed,
        vec![(
            "multi".to_string(),
            "1.0.0,2.0.0".to_string(),
            "2.0.0,3.0.0".to_string(),
        )]
    );
    assert_eq!(
        result.removed,
        vec![("removed".to_string(), "1.0.0".to_string())]
    );
    assert_eq!(
        result.added,
        vec![("added".to_string(), "1.0.0".to_string())]
    );
}

#[test]
fn cargo_tree_uses_the_compared_head_and_preserves_both_ancestries() {
    let fixture = LockFixture::new();
    let base = fixture.0.join("base");
    let head = fixture.0.join("head");
    write_workspace(&base, "1.0.0");
    write_workspace(&head, "2.0.0");
    let base_lock = fs::read(base.join("Cargo.lock")).unwrap();
    let head_lock = fs::read(head.join("Cargo.lock")).unwrap();
    let direct = HashSet::from([
        "lock-root".to_string(),
        "first".to_string(),
        "second".to_string(),
    ]);

    let result = diff_worktrees(&base, &head, &direct).unwrap();

    assert_eq!(
        result.changed,
        vec![(
            "shared".to_string(),
            "1.0.0".to_string(),
            "2.0.0".to_string()
        )]
    );
    assert_eq!(result.via["shared"], "first,second");
    assert_eq!(fs::read(base.join("Cargo.lock")).unwrap(), base_lock);
    assert_eq!(fs::read(head.join("Cargo.lock")).unwrap(), head_lock);
}

#[test]
fn requested_lock_diff_fails_for_missing_lock_or_required_lock_update() {
    let fixture = LockFixture::new();
    let base = fixture.0.join("base");
    let head = fixture.0.join("head");
    write_workspace(&base, "1.0.0");
    write_workspace(&head, "1.0.0");
    let direct = HashSet::new();

    fs::remove_file(base.join("Cargo.lock")).unwrap();
    assert!(diff_worktrees(&base, &head, &direct).is_err());

    write_workspace(&base, "1.0.0");
    let lock = fs::read(head.join("Cargo.lock")).unwrap();
    fs::write(
        head.join("shared/Cargo.toml"),
        "[package]\nname = \"shared\"\nversion = \"2.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    assert!(diff_worktrees(&base, &head, &direct).is_err());
    assert_eq!(fs::read(head.join("Cargo.lock")).unwrap(), lock);
}

struct LockFixture(PathBuf);

impl LockFixture {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "zc-lock-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for LockFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_workspace(root: &Path, shared_version: &str) {
    for member in ["", "first", "second", "shared"] {
        fs::create_dir_all(root.join(member).join("src")).unwrap();
        fs::write(root.join(member).join("src/lib.rs"), "").unwrap();
    }
    fs::write(
        root.join("Cargo.toml"),
        concat!(
            "[package]\nname = \"lock-root\"\nversion = \"1.0.0\"\nedition = \"2021\"\n",
            "[dependencies]\nfirst = { path = \"first\" }\nsecond = { path = \"second\" }\n",
        ),
    )
    .unwrap();
    for member in ["first", "second"] {
        fs::write(
            root.join(member).join("Cargo.toml"),
            format!(
                "[package]\nname = \"{member}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\
                 [dependencies]\nshared = {{ path = \"../shared\" }}\n"
            ),
        )
        .unwrap();
    }
    fs::write(
        root.join("shared/Cargo.toml"),
        format!(
            "[package]\nname = \"shared\"\nversion = \"{shared_version}\"\nedition = \"2021\"\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("Cargo.lock"),
        format!(
            "version = 3\n\n\
             [[package]]\nname = \"first\"\nversion = \"1.0.0\"\ndependencies = [\"shared\"]\n\n\
             [[package]]\nname = \"lock-root\"\nversion = \"1.0.0\"\n\
             dependencies = [\"first\", \"second\"]\n\n\
             [[package]]\nname = \"second\"\nversion = \"1.0.0\"\ndependencies = [\"shared\"]\n\n\
             [[package]]\nname = \"shared\"\nversion = \"{shared_version}\"\n"
        ),
    )
    .unwrap();
}
