use std::path::PathBuf;

use super::{parse_args, Style, EXIT_USAGE};

fn parse(argv: &[&str]) -> Result<super::Args, (String, i32)> {
    parse_args(
        argv.iter().map(|arg| arg.to_string()).collect(),
        &Style::default(),
    )
}

#[test]
fn no_destination_is_requested_by_default() {
    let args = parse(&["main"]).expect("a bare ref parses");
    assert_eq!(args.opts.report_path, None);
    assert_eq!(args.opts.changelog_path, None);
    assert!(!args.opts.changelog_mode);
}

#[test]
fn changelog_file_takes_a_path_without_selecting_changelog_mode() {
    let args = parse(&["--changelog-file", "out/notes.md", "main"]).expect("the flag parses");
    assert_eq!(
        args.opts.changelog_path,
        Some(PathBuf::from("out/notes.md"))
    );
    assert!(!args.opts.changelog_mode);
    assert_eq!(args.positional, vec!["main".to_string()]);
}

#[test]
fn changelog_file_composes_with_the_other_destinations() {
    let args = parse(&[
        "--changelog",
        "--changelog-file",
        "notes.md",
        "--report",
        "report.json",
        "--fail-on",
        "none",
    ])
    .expect("the flags compose");
    assert_eq!(args.opts.changelog_path, Some(PathBuf::from("notes.md")));
    assert_eq!(args.opts.report_path, Some(PathBuf::from("report.json")));
    assert!(args.opts.changelog_mode);
    assert_eq!(args.opts.fail_on, super::FailOn::None);
}

#[test]
fn changelog_file_without_a_value_is_a_usage_error() {
    let Err((message, code)) = parse(&["--changelog-file"]) else {
        panic!("a flag with no value is rejected");
    };
    assert_eq!(code, EXIT_USAGE);
    assert_eq!(
        message,
        "error: '--changelog-file' needs a value (run with --help for usage)"
    );
}
