//! Arabic tokenization that keeps offsets into the untouched original text.
//!
//! Offsets here are indices into a `[char]` slice (Unicode code points). The public API
//! converts them to UTF-8 byte offsets at its boundary.

use crate::html::unescape_reference;
use crate::tables::Class;
use crate::unicode::{class, is_space, nfc, push_nfkc};

/// Characters that never change lexical identity.
pub(crate) const IGNORED: [char; 6] = [
    'ـ', '\u{200C}', '\u{200D}', '\u{FEFF}', '\u{200E}', '\u{200F}',
];

/// Quotation, citation and sentence delimiters that separate matching blocks.
pub(crate) const HARD: [char; 23] = [
    '"', '“', '”', '«', '»', '﴿', '﴾', '(', ')', '[', ']', '{', '}', '<', '>', '.', '!', '?', '؟',
    ';', '؛', '‘', '’',
];

/// Hamza and madda can be decomposed components of a letter, unlike vowel marks.
const COMPONENTS: [char; 3] = ['\u{0653}', '\u{0654}', '\u{0655}'];

/// Unicode does not compatibility-decompose the Bismillah ligature.
pub(crate) const BASMALA_LIGATURE: char = '\u{FDFD}';
pub(crate) const BASMALA: &str = "بسم الله الرحمن الرحيم";

/// The letter folding applied to normalized words.
pub(crate) fn fold(c: char) -> char {
    match c {
        'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
        'ة' => 'ه',
        'ى' | 'ی' => 'ي',
        'ک' => 'ك',
        other => other,
    }
}

/// A normalized word and its half-open code-point interval in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) norm: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) block: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Letter,
    Mark,
    Ignore,
    Barrier,
    Space,
}

fn kind(c: char) -> Kind {
    if IGNORED.contains(&c) {
        return Kind::Ignore;
    }
    match class(c) {
        Class::Mark => Kind::Mark,
        Class::ArabicLetter => Kind::Letter,
        Class::OtherLetter | Class::NumberOrSymbol => Kind::Barrier,
        Class::Unassigned | Class::Other if HARD.contains(&c) => Kind::Barrier,
        Class::Unassigned | Class::Other => Kind::Space,
    }
}

/// `_expanded`: NFKC with letter folding, and the Bismillah ligature spelled out.
fn expand(c: char, out: &mut Vec<char>) {
    out.clear();
    if c == BASMALA_LIGATURE {
        out.extend(BASMALA.chars());
    } else {
        push_nfkc(c, out);
        for folded in out.iter_mut() {
            *folded = fold(*folded);
        }
    }
}

/// Spans of markup tags and character references, matched like CPython's
/// `re.finditer(r"<[/!A-Za-z_][^>]*>|&(?:#[0-9]+|#x[0-9a-fA-F]+|[A-Za-z]+);", text)`.
pub(crate) fn wrappers(text: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    // The first '>' at or after each index, computed only when tags are possible.
    let next_close: Vec<usize> = if text.contains(&'<') {
        let mut next = vec![usize::MAX; text.len() + 1];
        for i in (0..text.len()).rev() {
            next[i] = if text[i] == '>' { i } else { next[i + 1] };
        }
        next
    } else {
        Vec::new()
    };
    let mut i = 0;
    while i < text.len() {
        let end = match text[i] {
            '<' => tag_end(text, &next_close, i),
            '&' => reference_end(text, i),
            _ => None,
        };
        if let Some(end) = end {
            spans.push((i, end));
            i = end;
        } else {
            i += 1;
        }
    }
    spans
}

fn tag_end(text: &[char], next_close: &[usize], at: usize) -> Option<usize> {
    let first = *text.get(at + 1)?;
    if !(first == '/' || first == '!' || first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    // `usize::MAX` marks "no '>' follows", and becomes `None` here.
    next_close[at + 2].checked_add(1)
}

fn run(text: &[char], from: usize, accept: impl Fn(char) -> bool) -> usize {
    text[from..]
        .iter()
        .position(|&c| !accept(c))
        .map_or(text.len(), |length| from + length)
}

fn reference_end(text: &[char], at: usize) -> Option<usize> {
    let semicolon = |end: usize| (end < text.len() && text[end] == ';').then_some(end + 1);
    let next = *text.get(at + 1)?;
    if next == '#' {
        let digits = run(text, at + 2, |c| c.is_ascii_digit());
        if digits > at + 2
            && let Some(end) = semicolon(digits)
        {
            return Some(end);
        }
        if text.get(at + 2) == Some(&'x') {
            let hex = run(text, at + 3, |c| c.is_ascii_hexdigit());
            if hex > at + 3 {
                return semicolon(hex);
            }
        }
        return None;
    }
    if next.is_ascii_alphabetic() {
        return semicolon(run(text, at + 1, |c| c.is_ascii_alphabetic()));
    }
    None
}

struct Tokenizer {
    tokens: Vec<Token>,
    letters: Vec<char>,
    start: usize,
    end: usize,
    block: usize,
    newlines: usize,
}

impl Tokenizer {
    fn flush(&mut self) {
        if !self.letters.is_empty() {
            self.tokens.push(Token {
                norm: self.letters.drain(..).collect(),
                start: self.start,
                end: self.end,
                block: self.block,
            });
        }
    }

    fn consume(&mut self, c: char, left: usize, right: usize) {
        match kind(c) {
            Kind::Letter => {
                if self.letters.is_empty() {
                    self.start = left;
                }
                self.letters.push(c);
                self.end = right;
                self.newlines = 0;
            }
            Kind::Mark | Kind::Ignore => {
                if let Some(&last) = self.letters.last() {
                    if COMPONENTS.contains(&c) {
                        let composed: Vec<char> = nfc(&[last, c]).chars().map(fold).collect();
                        if let [single] = composed[..]
                            && kind(single) == Kind::Letter
                        {
                            *self.letters.last_mut().expect("nonempty") = single;
                        }
                    }
                    self.end = right;
                }
            }
            other => {
                self.flush();
                if other == Kind::Barrier {
                    self.block += 1;
                    self.newlines = 0;
                } else if c == '\n' {
                    self.newlines += 1;
                    if self.newlines == 2 {
                        self.block += 1;
                    }
                } else if !is_space(c) {
                    self.newlines = 0;
                }
            }
        }
    }

    fn consume_source(&mut self, c: char, left: usize, right: usize, buffer: &mut Vec<char>) {
        expand(c, buffer);
        for &expanded in buffer.iter() {
            self.consume(expanded, left, right);
        }
    }
}

/// Keep every Arabic word, including words attached to markup or punctuation.
///
/// Tags and citation or quotation delimiters separate matching blocks. Harakat, tatweel
/// and shaping controls do not change lexical identity. Compatibility ligatures and HTML
/// entities keep the interval of their original spelling. No global normalization or
/// whitespace reconstruction is applied to the input.
pub(crate) fn tokenize(text: &[char]) -> Vec<Token> {
    let mut state = Tokenizer {
        tokens: Vec::new(),
        letters: Vec::new(),
        start: 0,
        end: 0,
        block: 0,
        newlines: 0,
    };
    let mut buffer = Vec::new();
    let mut position = 0;
    for (start, end) in wrappers(text) {
        for (i, &c) in text.iter().enumerate().take(start).skip(position) {
            state.consume_source(c, i, i + 1, &mut buffer);
        }
        let raw = &text[start..end];
        if raw[0] == '<' {
            state.flush();
            state.block += 1;
        } else {
            let decoded = unescape_reference(raw);
            if decoded == raw {
                // Unknown entities must not join neighboring words.
                state.flush();
                state.block += 1;
            } else {
                let mut expanded = Vec::new();
                for c in decoded {
                    state.consume_source(c, start, end, &mut expanded);
                }
            }
        }
        position = end;
    }
    for (i, &c) in text.iter().enumerate().skip(position) {
        state.consume_source(c, i, i + 1, &mut buffer);
    }
    state.flush();
    state.tokens
}

/// Normalized words joined by single spaces.
pub(crate) fn normalize(text: &[char]) -> String {
    tokenize(text)
        .into_iter()
        .map(|token| token.norm)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    fn words(text: &str) -> Vec<(String, usize, usize, usize)> {
        tokenize(&chars(text))
            .into_iter()
            .map(|t| (t.norm, t.start, t.end, t.block))
            .collect()
    }

    #[test]
    fn wrapper_spans_follow_the_regular_expression() {
        let spans = |text: &str| wrappers(&chars(text));
        assert_eq!(spans("<b>x</b>"), [(0, 3), (4, 8)]);
        assert_eq!(spans("a < b"), []);
        assert_eq!(spans("<a"), []);
        assert_eq!(spans("<a <b>"), [(0, 6)]);
        assert_eq!(spans("&amp;&#12;&#x1F;"), [(0, 5), (5, 10), (10, 16)]);
        assert_eq!(spans("&#X41; &#12x; &; &a1; &#x;"), []);
        assert_eq!(spans("&&amp;"), [(1, 6)]);
    }

    #[test]
    fn marks_tatweel_and_entities_keep_original_offsets() {
        assert_eq!(
            words("بِسْمِ اللّـهِ"),
            [("بسم".into(), 0, 6, 0), ("الله".into(), 7, 14, 0)]
        );
        assert_eq!(words("أ&#1576;"), [("اب".into(), 0, 8, 0)]);
        assert_eq!(
            words("قال: «هو» <b>نعم</b>"),
            [
                ("قال".into(), 0, 3, 0),
                ("هو".into(), 6, 8, 1),
                ("نعم".into(), 13, 16, 3)
            ]
        );
        // A decomposed hamza composes with its letter before folding.
        assert_eq!(words("ي\u{0654}"), [("ئ".into(), 0, 2, 0)]);
        assert_eq!(words("ا\u{0654}"), [("ا".into(), 0, 2, 0)]);
        // Two consecutive newlines start a new block; one does not.
        assert_eq!(
            words("أ\nب\n\nج"),
            [
                ("ا".into(), 0, 1, 0),
                ("ب".into(), 2, 3, 0),
                ("ج".into(), 5, 6, 1)
            ]
        );
        assert_eq!(normalize(&chars("\u{FDFD}")), BASMALA);
    }
}
