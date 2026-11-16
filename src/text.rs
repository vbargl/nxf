//! Text helpers shared by the presentation and the tool wrappers.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Removes terminal escape sequences: CSI, OSC and two-character escapes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' {
                        chars.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Removes C0 and C1 control characters.
pub fn strip_controls(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_control()).collect()
}

/// Removes control characters except tab and newline, after escape sequences.
pub fn plain(text: &str) -> String {
    strip_ansi(text)
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\t' || *ch == '\n')
        .collect()
}

/// Fits text in terminal cells, ending with an ellipsis when clipped.
pub fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut prefix = String::new();
    for grapheme in text.graphemes(true) {
        if prefix.width() + grapheme.width() >= width {
            break;
        }
        prefix.push_str(grapheme);
    }
    prefix.push('…');
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escape_sequences() {
        assert_eq!(strip_ansi("\u{1b}[1;31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
        assert_eq!(strip_ansi("\u{1b}[2A\r\u{1b}[2Kline"), "\rline");
    }

    #[test]
    fn fits_in_cells() {
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit("abcdefghij", 5), "abcd…");
        assert!(fit("编译🦀👩‍💻é long", 6).width() <= 6);
        assert_eq!(fit("abc", 1), "…");
    }
}
