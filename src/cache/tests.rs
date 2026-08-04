use super::sha1_short;

#[test]
fn sha1_is_lowercase_and_truncated_to_twelve_hex_digits() {
    assert_eq!(sha1_short(b""), "da39a3ee5e6b");
    assert_eq!(sha1_short(b"abc"), "a9993e364706");
}
