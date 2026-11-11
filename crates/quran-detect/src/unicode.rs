//! CPython 3.13 character semantics, frozen at Unicode 15.1.
//!
//! Classification and whitespace come from generated tables. Normalization uses the
//! `unicode-normalization` crate, whose decomposition, combining-class and composition
//! data are stable for every character assigned in Unicode 15.1. Characters unassigned
//! in 15.1 are never normalized, exactly as CPython 3.13 leaves them unchanged.

use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfkc_quick};

use crate::tables::{CLASS_RANGES, Class, SPACES};

/// The generated class of a character.
pub(crate) fn class(c: char) -> Class {
    let point = u32::from(c);
    let after = CLASS_RANGES.partition_point(|&(start, _)| start <= point);
    // The table's first range starts at U+0000, so `after` is at least one.
    CLASS_RANGES[after - 1].1
}

/// CPython `str.isspace()` for one character.
pub(crate) fn is_space(c: char) -> bool {
    SPACES.binary_search(&u32::from(c)).is_ok()
}

/// CPython `str.isspace()` for a string: non-empty and entirely whitespace.
pub(crate) fn is_all_space(text: &[char]) -> bool {
    !text.is_empty() && text.iter().all(|&c| is_space(c))
}

/// Append `unicodedata.normalize("NFKC", c)` for one character.
pub(crate) fn push_nfkc(c: char, out: &mut Vec<char>) {
    if class(c) == Class::Unassigned || is_nfkc_quick(std::iter::once(c)) == IsNormalized::Yes {
        out.push(c);
    } else {
        out.extend(std::iter::once(c).nfkc());
    }
}

/// `unicodedata.normalize("NFC", text)` for characters assigned in Unicode 15.1.
///
/// Every caller passes token text: such text contains only assigned characters or ASCII
/// entity spellings, because an unassigned character always ends a token.
pub(crate) fn nfc(text: &[char]) -> String {
    text.iter().copied().nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_every_code_point_in_order() {
        assert_eq!(CLASS_RANGES[0].0, 0);
        assert!(CLASS_RANGES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(CLASS_RANGES.windows(2).all(|pair| pair[0].1 != pair[1].1));
        assert_eq!(class('ب'), Class::ArabicLetter);
        assert_eq!(class('\u{064E}'), Class::Mark);
        assert_eq!(class('a'), Class::OtherLetter);
        assert_eq!(class('٣'), Class::NumberOrSymbol);
        assert_eq!(class('،'), Class::Other);
        assert_eq!(class('\u{0378}'), Class::Unassigned);
        assert_eq!(class('\u{10FFFF}'), Class::Unassigned);
        // Small waw and yeh are letters by category; source mapping treats them as marks.
        assert_eq!(class('\u{06E5}'), Class::ArabicLetter);
    }

    #[test]
    fn whitespace_matches_python_rather_than_rust() {
        for c in [
            '\t', '\n', '\u{0B}', '\u{1C}', '\u{1F}', ' ', '\u{85}', '\u{A0}', '\u{3000}',
        ] {
            assert!(is_space(c), "{c:?}");
        }
        for c in ['\u{200B}', '\u{180E}', 'a', 'ـ'] {
            assert!(!is_space(c), "{c:?}");
        }
        assert!(!is_all_space(&[]));
        assert!(is_all_space(&[' ', '\n']));
        assert!(!is_all_space(&[' ', 'x']));
    }
}
