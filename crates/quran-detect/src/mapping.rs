//! Letter units with offsets into an unchanged source string, used to cut prefixes.
//!
//! This mirrors the reference `map_text` in its default (non-legacy) mode, but keeps only
//! what a prefix cut reads: unit boundaries, token splits and atomic source ranges. Unit
//! spellings and composition never change those boundaries, so they are not tracked.

use crate::html::unescape_reference;
use crate::tables::Class;
use crate::text::{BASMALA, BASMALA_LIGATURE, IGNORED, wrappers};
use crate::unicode::{class, push_nfkc};

/// Small high waw and yeh are marks here, although Unicode classifies them as letters.
const SMALL_MARKS: [char; 2] = ['\u{06E5}', '\u{06E6}'];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Unit {
    start: usize,
    end: usize,
}

/// A source string split into letter units and tokens.
pub(crate) struct Document {
    text: Vec<char>,
    tokens: Vec<Vec<Unit>>,
    atomic: Vec<(usize, usize)>,
}

/// The boundary between letter units could not be cut.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Uncuttable;

impl Document {
    pub(crate) fn token_count(&self) -> usize {
        self.tokens.len()
    }

    /// The source offset before letter `index` of the first token, with attached marks.
    pub(crate) fn cut_first(&self, index: usize) -> Result<usize, Uncuttable> {
        let units = &self.tokens[0];
        if index > units.len() {
            return Err(Uncuttable);
        }
        if index == 0 {
            return Ok(units[0].start);
        }
        if index == units.len() {
            return Ok(units[units.len() - 1].end);
        }
        if units[index - 1].end > units[index].start {
            // A ligature or an entity cannot be split.
            return Err(Uncuttable);
        }
        Ok(units[index].start)
    }

    /// Whether the source may be split at `position` without breaking a character cluster.
    pub(crate) fn legal_cut(&self, position: usize) -> bool {
        if position > self.text.len() {
            return false;
        }
        if 0 < position && position < self.text.len() {
            let c = self.text[position];
            if class(c) == Class::Mark
                || IGNORED.contains(&c)
                || SMALL_MARKS.contains(&c)
                || (self.text[position - 1] == '\r' && c == '\n')
            {
                return false;
            }
        }
        // Ranges are sorted by start; find the last one starting at or before `position`.
        let after = self.atomic.partition_point(|&(start, _)| start <= position);
        !(after > 0 && {
            let (start, end) = self.atomic[after - 1];
            start < position && position < end
        })
    }
}

struct Mapper {
    tokens: Vec<Vec<Unit>>,
    units: Vec<Unit>,
}

impl Mapper {
    fn flush(&mut self) {
        if !self.units.is_empty() {
            self.tokens.push(std::mem::take(&mut self.units));
        }
    }

    fn consume(&mut self, c: char, start: usize, end: usize) {
        let class = class(c);
        if IGNORED.contains(&c) || class == Class::Mark || SMALL_MARKS.contains(&c) {
            if let Some(last) = self.units.last_mut() {
                last.end = end;
            }
            return;
        }
        if class == Class::ArabicLetter {
            self.units.push(Unit { start, end });
            return;
        }
        // Barriers and spaces both end the current token here; blocks are not needed.
        self.flush();
    }

    /// Script-variant folding (Farsi yeh and keheh) maps letters to letters, so it is omitted.
    fn expand(&mut self, source: char, start: usize, end: usize, buffer: &mut Vec<char>) {
        buffer.clear();
        if source == BASMALA_LIGATURE {
            buffer.extend(BASMALA.chars());
        } else {
            push_nfkc(source, buffer);
        }
        let point = u32::from(source);
        if (0xFE70..=0xFE7F).contains(&point) || (0xFC5E..=0xFC63).contains(&point) {
            // These presentation forms encode a mark, not a word separator.
            buffer.retain(|&c| c != ' ' && c != 'ـ');
        }
        for &c in buffer.iter() {
            self.consume(c, start, end);
        }
    }
}

/// Map `text` into letter units, tokens and atomic (entity, tag or ligature) ranges.
pub(crate) fn map_text(text: &[char]) -> Document {
    let mut mapper = Mapper {
        tokens: Vec::new(),
        units: Vec::new(),
    };
    let mut atomic = Vec::new();
    let mut buffer = Vec::new();
    let mut position = 0;
    for (start, end) in wrappers(text) {
        for (i, &c) in text.iter().enumerate().take(start).skip(position) {
            mapper.expand(c, i, i + 1, &mut buffer);
        }
        let raw = &text[start..end];
        atomic.push((start, end));
        if raw[0] == '<' {
            mapper.flush();
        } else {
            let decoded = unescape_reference(raw);
            if decoded == raw {
                mapper.flush();
            } else {
                let mut expanded = Vec::new();
                for c in decoded {
                    mapper.expand(c, start, end, &mut expanded);
                }
            }
        }
        position = end;
    }
    for (i, &c) in text.iter().enumerate().skip(position) {
        mapper.expand(c, i, i + 1, &mut buffer);
    }
    mapper.flush();
    atomic.extend(
        mapper
            .tokens
            .iter()
            .flatten()
            .filter(|unit| unit.end - unit.start > 1)
            .map(|unit| (unit.start, unit.end)),
    );
    atomic.sort_unstable();
    atomic.dedup();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in atomic {
        match merged.last_mut() {
            Some(last) if start < last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    Document {
        text: text.to_vec(),
        tokens: mapper.tokens,
        atomic: merged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(text: &str) -> Document {
        map_text(&text.chars().collect::<Vec<_>>())
    }

    #[test]
    fn cuts_respect_marks_ligatures_and_entities() {
        let plain = document("وَقُلْ");
        assert_eq!(plain.token_count(), 1);
        assert_eq!(plain.cut_first(1), Ok(2));
        assert!(plain.legal_cut(2));
        assert!(!plain.legal_cut(1));
        // The lam-alef ligature is one source character holding two letters.
        let ligature = document("\u{FEFB}");
        assert_eq!(ligature.cut_first(1), Err(Uncuttable));
        let entity = document("&#1608;قل");
        assert_eq!(entity.cut_first(1), Ok(7));
        assert!(!entity.legal_cut(3));
        assert!(entity.legal_cut(7));
        assert_eq!(document("و قل").token_count(), 2);
        assert_eq!(plain.cut_first(9), Err(Uncuttable));
    }
}
