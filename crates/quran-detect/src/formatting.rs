//! Optional citation and bracket edits next to a match, in code-point offsets.

use serde::{Deserialize, Serialize};

use crate::corpus::Corpus;
use crate::model::EditKind;
use crate::render::{CharEdit, span};
use crate::unicode::is_space;

/// What to do with a bracket pair directly enclosing a match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Brackets {
    /// Leave existing brackets unchanged.
    #[default]
    Keep,
    /// Convert `«»`, `“”`, `()`, `[]`, `﴿﴾` or `{}` around the match to `{}`.
    Braces,
    /// Convert to `{}` and remove whitespace just inside the braces.
    BracesTrimmed,
}

impl Brackets {
    /// Every option.
    pub const ALL: [Self; 3] = [Self::Keep, Self::Braces, Self::BracesTrimmed];

    /// The stable name used by the command line and JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Braces => "braces",
            Self::BracesTrimmed => "braces-trimmed",
        }
    }
}

/// Optional edits around matches; the default changes nothing beyond the quotation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Formatting {
    /// Append a citation such as `[البقرة - ٢٥٥]` after each match.
    pub append_reference: bool,
    /// Bracket handling around each match.
    pub brackets: Brackets,
}

impl Formatting {
    pub(crate) fn is_noop(self) -> bool {
        !self.append_reference && self.brackets == Brackets::Keep
    }
}

/// A citation next to a match that differs from the one the library would add.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conflict {
    /// Always `reference_conflict`.
    pub kind: String,
    /// First byte of the existing citation.
    pub start: usize,
    /// Byte after the existing citation.
    pub end: usize,
    /// The existing citation text.
    pub existing: String,
    /// The citation the library would have added.
    pub expected: String,
}

/// A conflict in code-point offsets.
pub(crate) struct CharConflict {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) existing: String,
    pub(crate) expected: String,
}

fn closing(open: char) -> Option<char> {
    match open {
        '«' => Some('»'),
        '“' => Some('”'),
        '(' => Some(')'),
        '[' => Some(']'),
        '﴿' => Some('﴾'),
        '{' => Some('}'),
        _ => None,
    }
}

fn arabic_digits(number: u16) -> String {
    number
        .to_string()
        .chars()
        .map(|digit| {
            char::from_u32(0x0660 + digit.to_digit(10).expect("decimal"))
                .expect("Arabic-Indic digit")
        })
        .collect()
}

fn is_arabic_digit(c: char) -> bool {
    ('٠'..='٩').contains(&c)
}

/// The library citation for canonical positions `start..end`.
pub(crate) fn reference_label(corpus: &Corpus, start: usize, end: usize) -> String {
    let first = &corpus.verses[corpus.owner[start]];
    let last = &corpus.verses[corpus.owner[end - 1]];
    let mut numbers = arabic_digits(first.number);
    if last.number != first.number {
        numbers.push('-');
        numbers.push_str(&arabic_digits(last.number));
    }
    format!(
        "[{} - {numbers}]",
        corpus.names[usize::from(first.surah) - 1]
    )
}

/// `\[([^\[\]\r\n]+) - ([٠-٩]+)(?:-([٠-٩]+))?\]` anchored at `at`: `(end, name)`.
fn citation(text: &[char], at: usize) -> Option<(usize, String)> {
    if text.get(at) != Some(&'[') {
        return None;
    }
    // Only ']' can end the match: no part of the pattern accepts '[', ']', CR or LF.
    let close = at
        + 1
        + text[at + 1..]
            .iter()
            .position(|c| matches!(c, '[' | ']' | '\r' | '\n'))?;
    if text[close] != ']' {
        return None;
    }
    let content = &text[at + 1..close];
    let trailing = content
        .iter()
        .rev()
        .take_while(|&&c| is_arabic_digit(c))
        .count();
    if trailing == 0 {
        return None;
    }
    let before = &content[..content.len() - trailing];
    // The greedy name takes the longest prefix, so prefer the form without a range.
    let name = if let Some(name) = before.strip_suffix(&[' ', '-', ' ']) {
        name
    } else {
        let before = before.strip_suffix(&['-'])?;
        let first = before
            .iter()
            .rev()
            .take_while(|&&c| is_arabic_digit(c))
            .count();
        if first == 0 {
            return None;
        }
        before[..before.len() - first].strip_suffix(&[' ', '-', ' '])?
    };
    if name.is_empty() {
        return None;
    }
    Some((close + 1, name.iter().collect()))
}

fn citation_at(text: &[char], mut offset: usize, corpus: &Corpus) -> Option<(usize, usize)> {
    while offset < text.len() && is_space(text[offset]) {
        offset += 1;
    }
    let (end, name) = citation(text, offset)?;
    corpus.names.contains(&name).then_some((offset, end))
}

/// Edits and conflicts for a match at `start..end` with canonical positions `reference`.
pub(crate) fn formatting_edits(
    text: &[char],
    (start, end): (usize, usize),
    reference: (usize, usize),
    corpus: &Corpus,
    formatting: Formatting,
) -> (Vec<CharEdit>, Vec<CharConflict>) {
    let mut left = start.checked_sub(1);
    while let Some(index) = left
        && is_space(text[index])
    {
        left = index.checked_sub(1);
    }
    let mut right = end;
    while right < text.len() && is_space(text[right]) {
        right += 1;
    }
    let owned =
        left.is_some_and(|left| right < text.len() && closing(text[left]) == Some(text[right]));
    let mut edits = Vec::new();
    let mut conflicts = Vec::new();
    let mut edit = |a: usize, b: usize, after: &str, kind: EditKind| {
        let before: String = span(text, a, b).iter().collect();
        if before != after {
            edits.push(CharEdit {
                start: a,
                end: b,
                before,
                after: after.to_owned(),
                kinds: vec![kind],
            });
        }
    };
    let normalize = formatting.brackets != Brackets::Keep && owned;
    if normalize {
        let left = left.expect("owned brackets have a left side");
        if formatting.brackets == Brackets::BracesTrimmed {
            edit(left, start, "{", EditKind::Bracket);
            edit(end, right + 1, "}", EditKind::Bracket);
        } else {
            edit(left, left + 1, "{", EditKind::Bracket);
            edit(right, right + 1, "}", EditKind::Bracket);
        }
    }
    if formatting.append_reference {
        let label = reference_label(corpus, reference.0, reference.1);
        let mut existing = citation_at(text, end, corpus);
        if existing.is_none() && owned {
            existing = citation_at(text, right + 1, corpus);
        }
        if let Some((a, b)) = existing {
            let found: String = text[a..b].iter().collect();
            if found != label {
                conflicts.push(CharConflict {
                    start: a,
                    end: b,
                    existing: found,
                    expected: label,
                });
            }
        } else {
            let at = if normalize { right + 1 } else { end };
            edit(at, at, &format!(" {label}"), EditKind::Reference);
        }
    }
    (edits, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::corpus;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn citations_follow_the_greedy_pattern() {
        let parse = |text: &str| citation(&chars(text), 0);
        assert_eq!(parse("[البقرة - ٢٥٥]"), Some((14, "البقرة".into())));
        assert_eq!(parse("[الإخلاص - ١-٢] بعد"), Some((15, "الإخلاص".into())));
        assert_eq!(parse("[a - b - ١]"), Some((11, "a - b".into())));
        assert_eq!(parse("[ - ١]"), None);
        assert_eq!(parse("[البقرة - 255]"), None);
        assert_eq!(parse("[البقرة - ٢٥٥"), None);
        assert_eq!(parse("[البقرة\n - ٢٥٥]"), None);
        assert_eq!(parse("[X - -٢]"), None);
        assert_eq!(parse("[X -٢]"), None);
        assert_eq!(parse("[X - ١-]"), None);
    }

    #[test]
    fn labels_use_arabic_digits_and_ranges() {
        let corpus = corpus();
        let verse = |surah: u16, number: u16| {
            corpus
                .verses
                .iter()
                .find(|v| (v.surah, v.number) == (surah, number))
                .unwrap()
                .clone()
        };
        let kursi = verse(2, 255);
        assert_eq!(
            reference_label(corpus, kursi.offset, kursi.offset + 2),
            "[البقرة - ٢٥٥]"
        );
        let (one, two) = (verse(112, 1), verse(112, 2));
        assert_eq!(
            reference_label(corpus, one.offset, two.end()),
            "[الإخلاص - ١-٢]"
        );
    }
}
