use std::fs;
use std::path::{Path, PathBuf};

use super::{check_dir, write};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zc-report-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the system temp directory is writable");
    dir
}

#[test]
fn writing_a_report_creates_the_file_with_the_document() {
    let dir = scratch("write");
    let path = dir.join("report.json");

    assert_eq!(write("report", &path, "{\"verdict\":\"ok\"}"), Ok(()));
    assert_eq!(
        fs::read_to_string(&path).expect("the report was just written"),
        "{\"verdict\":\"ok\"}"
    );

    fs::remove_dir_all(&dir).expect("the scratch directory is removable");
}

#[test]
fn an_existing_report_is_replaced() {
    let dir = scratch("replace");
    let path = dir.join("report.json");

    write("report", &path, "{\"verdict\":\"breaking\"}").expect("the directory exists");
    write("report", &path, "{}").expect("the directory exists");

    assert_eq!(
        fs::read_to_string(&path).expect("the report was just written"),
        "{}"
    );

    fs::remove_dir_all(&dir).expect("the scratch directory is removable");
}

#[test]
fn a_missing_directory_is_rejected_before_the_analysis() {
    let dir = scratch("missing");
    let missing = dir.join("nested");

    assert_eq!(
        check_dir("report", &missing.join("report.json")),
        Err(format!(
            "report directory '{}' does not exist",
            missing.display()
        ))
    );
    assert_eq!(check_dir("report", &dir.join("report.json")), Ok(()));

    fs::remove_dir_all(&dir).expect("the scratch directory is removable");
}

#[test]
fn a_bare_file_name_writes_into_the_current_directory() {
    assert_eq!(check_dir("report", Path::new("report.json")), Ok(()));
}

#[test]
fn the_label_names_the_document_in_both_diagnostics() {
    let dir = scratch("label");
    let missing = dir.join("nested");

    assert_eq!(
        check_dir("changelog", &missing.join("changelog.md")),
        Err(format!(
            "changelog directory '{}' does not exist",
            missing.display()
        ))
    );
    let failure = write("changelog", &missing.join("changelog.md"), "## crate\n")
        .expect_err("the directory is missing");
    assert!(
        failure.starts_with("failed to write changelog '"),
        "unexpected diagnostic: {failure}"
    );

    fs::remove_dir_all(&dir).expect("the scratch directory is removable");
}
