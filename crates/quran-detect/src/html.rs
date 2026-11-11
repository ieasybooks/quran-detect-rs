//! CPython 3.13 `html.unescape` for the single references the tokenizer recognizes.
//!
//! The tokenizer only passes `&#DIGITS;`, `&#xHEX;` or `&LETTERS;`. CPython's
//! `_charref` substitution then matches the whole reference once, except that a name
//! longer than 32 letters is matched by its first 32 letters and the rest is kept.

use crate::tables::{ENTITIES, INVALID_CHARREFS, INVALID_CODEPOINTS};

/// Decode one wrapper-shaped reference, such as `&amp;`, `&#1576;` or `&#x628;`.
pub(crate) fn unescape_reference(raw: &[char]) -> Vec<char> {
    debug_assert!(raw.len() >= 3 && raw[0] == '&' && raw[raw.len() - 1] == ';');
    let body = &raw[1..];
    if body[0] == '#' {
        let (digits, radix) = if body[1] == 'x' {
            (&body[2..body.len() - 1], 16)
        } else {
            (&body[1..body.len() - 1], 10)
        };
        return numeric(saturating_number(digits, radix));
    }
    let letters = &body[..body.len() - 1];
    // `[^\t\n\f <&#;]{1,32};?` takes at most 32 letters, plus the semicolon only if it follows.
    let (name, rest) = if letters.len() <= 32 {
        (body, &[][..])
    } else {
        (&letters[..32], &body[32..])
    };
    let name: String = name.iter().collect();
    let mut out: Vec<char> = if let Some(value) = entity(&name) {
        value.chars().collect()
    } else if let Some((value, tail)) = (2..name.len())
        .rev()
        .find_map(|length| entity(&name[..length]).map(|value| (value, &name[length..])))
    {
        value.chars().chain(tail.chars()).collect()
    } else {
        std::iter::once('&').chain(name.chars()).collect()
    };
    out.extend_from_slice(rest);
    out
}

fn entity(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by(|(candidate, _)| (*candidate).cmp(name))
        .ok()
        .map(|index| ENTITIES[index].1)
}

/// Every value above U+10FFFF has the same outcome, so saturation preserves behavior.
///
/// CPython raises `ValueError` for a decimal reference longer than 4,300 digits (its
/// integer-conversion limit), which crashes the reference tokenizer; here such a
/// reference decodes to U+FFFD like any other out-of-range value.
fn saturating_number(digits: &[char], radix: u32) -> u64 {
    digits.iter().fold(0_u64, |number, digit| {
        let value = u64::from(digit.to_digit(radix).expect("wrapper pattern digits"));
        number
            .checked_mul(u64::from(radix))
            .and_then(|number| number.checked_add(value))
            .unwrap_or(u64::MAX)
    })
}

fn numeric(number: u64) -> Vec<char> {
    if let Ok(index) = INVALID_CHARREFS.binary_search_by_key(&number, |&(n, _)| u64::from(n)) {
        return vec![INVALID_CHARREFS[index].1];
    }
    if (0xD800..=0xDFFF).contains(&number) || number > 0x10_FFFF {
        return vec!['\u{FFFD}'];
    }
    let point = u32::try_from(number).expect("at most U+10FFFF");
    if INVALID_CODEPOINTS.binary_search(&point).is_ok() {
        return Vec::new();
    }
    vec![char::from_u32(point).expect("a scalar value outside the surrogate range")]
}

#[cfg(test)]
mod tests {
    use super::unescape_reference;

    fn decode(raw: &str) -> String {
        unescape_reference(&raw.chars().collect::<Vec<_>>())
            .into_iter()
            .collect()
    }

    #[test]
    fn named_numeric_and_legacy_prefix_references() {
        assert_eq!(decode("&amp;"), "&");
        assert_eq!(decode("&#1576;"), "ب");
        assert_eq!(decode("&#x628;"), "ب");
        assert_eq!(decode("&notit;"), "¬it;");
        assert_eq!(decode("&nosuchentity;"), "&nosuchentity;");
        assert_eq!(decode("&#128;"), "€");
        assert_eq!(decode("&#xD800;"), "\u{FFFD}");
        assert_eq!(decode("&#1;"), "");
        assert_eq!(decode("&#99999999999999999999999999;"), "\u{FFFD}");
    }
}
