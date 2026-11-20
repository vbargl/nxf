//! Tests for src/text.rs: escape-sequence removal and clipping to cells.

use nxf::text::{fit, last_line, strip_ansi};
use unicode_width::UnicodeWidthStr;

/// CSI and OSC sequences are removed; the text and a carriage return stay.
#[test]
fn strips_escape_sequences() {
    assert_eq!(strip_ansi("\u{1b}[1;31mred\u{1b}[0m"), "red");
    assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
    assert_eq!(strip_ansi("\u{1b}[2A\r\u{1b}[2Kline"), "\rline");
}

/// Text that is too wide ends with an ellipsis and never exceeds the width,
/// also when its characters are wide or joined.
#[test]
fn fits_in_cells() {
    assert_eq!(fit("short", 10), "short");
    assert_eq!(fit("abcdefghij", 5), "abcd…");
    assert!(fit("编译🦀👩‍💻é long", 6).width() <= 6);
    assert_eq!(fit("abc", 1), "…");
}

/// The last line is the last one with something on it, trimmed.
#[test]
fn last_lines() {
    assert_eq!(last_line("one\n  two  \n\n \n"), Some("two".to_string()));
    assert_eq!(last_line("only"), Some("only".to_string()));
    assert_eq!(last_line(""), None);
    assert_eq!(last_line("\n \n"), None);
}
