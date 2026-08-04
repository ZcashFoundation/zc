use super::parse_type_kind_line;

#[test]
fn parses_type_names_after_declaration_keyword() {
    assert_eq!(
        parse_type_kind_line("deadbeef:path/file.rs:pub struct Widget<T> {"),
        Some(("Widget".to_string(), "struct".to_string()))
    );
    assert_eq!(
        parse_type_kind_line("deadbeef:path/file.rs:    pub enum State{Ready}"),
        Some(("State".to_string(), "enum".to_string()))
    );
    assert_eq!(
        parse_type_kind_line("deadbeef:path/file.rs:pub trait Service: Send"),
        Some(("Service".to_string(), "trait".to_string()))
    );
    assert_eq!(
        parse_type_kind_line("deadbeef:path/file.rs:pub union Bits;"),
        Some(("Bits".to_string(), "union".to_string()))
    );
}

#[test]
fn rejects_lines_without_a_name_after_the_keyword() {
    assert_eq!(parse_type_kind_line("path.rs:pub struct"), None);
    assert_eq!(parse_type_kind_line("path.rs:pub fn structish()"), None);
    assert_eq!(parse_type_kind_line("path.rs:pub enum {"), None);
}
