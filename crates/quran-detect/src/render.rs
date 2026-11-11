//! Validated word edits in source coordinates, checked by re-tokenizing the result.

use crate::alignment::{Candidate, OpKind, distance_is_one};
use crate::corpus::Corpus;
use crate::model::EditKind;
use crate::text::{Token, tokenize};
use crate::unicode::{is_all_space, nfc};

/// An edit in code-point offsets, before conversion to the public byte-offset form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CharEdit {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) before: String,
    pub(crate) after: String,
    pub(crate) kinds: Vec<EditKind>,
}

/// An alignment that cannot be rendered as exact source edits.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Issue;

pub(crate) fn edit_kind(kind: OpKind) -> EditKind {
    match kind {
        OpKind::Exact => EditKind::Exact,
        OpKind::Extra => EditKind::Extra,
        OpKind::Join => EditKind::Join,
        OpKind::Missing => EditKind::Missing,
        OpKind::Split => EditKind::Split,
        OpKind::Substitution => EditKind::Substitution,
        OpKind::Typo => EditKind::Typo,
    }
}

/// `text[a:b]` with Python's empty result for reversed bounds.
pub(crate) fn span(text: &[char], a: usize, b: usize) -> &[char] {
    if a < b { &text[a..b] } else { &[] }
}

fn same(text: &[char], expected: &str) -> bool {
    text.iter().copied().eq(expected.chars())
}

/// Apply ordered, nonoverlapping edits whose offsets are relative to `shift`.
pub(crate) fn splice(text: &[char], edits: &[CharEdit], shift: usize) -> Result<Vec<char>, Issue> {
    let mut out = Vec::with_capacity(text.len());
    let mut cursor = 0;
    for edit in edits {
        let (Some(a), Some(b)) = (edit.start.checked_sub(shift), edit.end.checked_sub(shift))
        else {
            return Err(Issue);
        };
        if !(cursor <= a && a <= b && b <= text.len() && same(&text[a..b], &edit.before)) {
            return Err(Issue);
        }
        out.extend_from_slice(&text[cursor..a]);
        out.extend(edit.after.chars());
        cursor = b;
    }
    out.extend_from_slice(&text[cursor..]);
    Ok(out)
}

pub(crate) struct Renderer<'c> {
    pub(crate) corpus: &'c Corpus,
    pub(crate) canonical: &'c [String],
}

struct Component<'c> {
    start: usize,
    end: usize,
    words: Vec<&'c str>,
    kinds: Vec<OpKind>,
}

impl<'c> Renderer<'c> {
    fn validate_alignment(&self, candidate: &Candidate, tokens: &[Token]) -> Result<(), Issue> {
        let words = &self.corpus.words;
        let (mut q, mut r) = (candidate.query_start, candidate.reference_start);
        if !(q < candidate.query_end
            && candidate.query_end <= tokens.len()
            && r < candidate.reference_end
            && candidate.reference_end <= words.len())
        {
            return Err(Issue);
        }
        for p in r..candidate.reference_end - 1 {
            if !self.corpus.connected(signed(p), signed(p + 1)) {
                return Err(Issue);
            }
        }
        for op in &candidate.operations {
            if (op.query_start, op.reference_start) != (q, r) {
                return Err(Issue);
            }
            let shape = match op.kind {
                OpKind::Exact | OpKind::Typo | OpKind::Substitution => (1, 1),
                OpKind::Join => (1, 2),
                OpKind::Split => (2, 1),
                OpKind::Missing => (0, 1),
                OpKind::Extra => (1, 0),
            };
            if (op.query_end.checked_sub(q), op.reference_end.checked_sub(r))
                != (Some(shape.0), Some(shape.1))
            {
                return Err(Issue);
            }
            if !(op.query_end <= candidate.query_end && op.reference_end <= candidate.reference_end)
            {
                return Err(Issue);
            }
            let query: Vec<&str> = tokens[q..op.query_end]
                .iter()
                .map(|t| t.norm.as_str())
                .collect();
            let reference: Vec<&str> = words[r..op.reference_end]
                .iter()
                .map(String::as_str)
                .collect();
            let valid = match op.kind {
                OpKind::Exact => query == reference,
                OpKind::Typo => distance_is_one(query[0], reference[0]),
                OpKind::Join | OpKind::Split => query.concat() == reference.concat(),
                OpKind::Missing | OpKind::Extra | OpKind::Substitution => true,
            };
            if !valid {
                return Err(Issue);
            }
            (q, r) = (op.query_end, op.reference_end);
        }
        if (q, r) != (candidate.query_end, candidate.reference_end) {
            return Err(Issue);
        }
        let errors = candidate
            .operations
            .iter()
            .filter(|op| op.kind != OpKind::Exact)
            .count();
        if candidate.edits != errors {
            return Err(Issue);
        }
        Ok(())
    }

    /// The source edits that render `candidate` with canonical words.
    pub(crate) fn candidate_edits(
        &self,
        text: &[char],
        tokens: &[Token],
        candidate: &Candidate,
    ) -> Result<Vec<CharEdit>, Issue> {
        self.validate_alignment(candidate, tokens)?;
        let mut components: Vec<Component<'c>> = Vec::new();
        for op in &candidate.operations {
            let (q0, q1) = (op.query_start, op.query_end);
            let words: Vec<&'c str> = self.canonical[op.reference_start..op.reference_end]
                .iter()
                .map(String::as_str)
                .collect();
            let (mut a, b);
            if q0 == q1 {
                if !(candidate.query_start < q0 && q0 < candidate.query_end) {
                    return Err(Issue); // an unanchored missing word
                }
                if !is_all_space(span(text, tokens[q0 - 1].end, tokens[q0].start)) {
                    return Err(Issue); // a missing word must not cross a separator
                }
                a = tokens[q0].start;
                b = a;
            } else {
                a = tokens[q0].start;
                b = tokens[q1 - 1].end;
                if op.kind == OpKind::Split
                    && !is_all_space(span(text, tokens[q0].end, tokens[q0 + 1].start))
                {
                    return Err(Issue); // joined words must not cross a separator
                }
                if op.kind == OpKind::Extra {
                    if !(candidate.query_start < q0 && q1 < candidate.query_end) {
                        return Err(Issue); // an unanchored extra word
                    }
                    let left = span(text, tokens[q0 - 1].end, a);
                    let right = span(text, b, tokens[q1].start);
                    if !is_all_space(left) || !is_all_space(right) {
                        return Err(Issue); // an extra word must not cross a separator
                    }
                    // Consume only an ordinary preceding space or tab, never a line break.
                    if left.iter().all(|&c| c == ' ' || c == '\t') {
                        a = tokens[q0 - 1].end;
                    }
                }
            }
            match components.last_mut() {
                Some(previous)
                    if a < previous.end
                        || (a == previous.end && (a == b || previous.start == a)) =>
                {
                    // Several logical tokens can share one ligature or entity source range.
                    previous.end = previous.end.max(b);
                    previous.words.extend(words);
                    previous.kinds.push(op.kind);
                }
                _ => components.push(Component {
                    start: a,
                    end: b,
                    words,
                    kinds: vec![op.kind],
                }),
            }
        }
        let mut edits = Vec::new();
        for component in components {
            let (a, b) = (component.start, component.end);
            let mut after = component.words.join(" ");
            if a == b {
                after.push(' ');
            }
            let before: String = span(text, a, b).iter().collect();
            if before != after {
                let mut kinds: Vec<EditKind> = component.kinds.into_iter().map(edit_kind).collect();
                kinds.sort_unstable();
                kinds.dedup();
                edits.push(CharEdit {
                    start: a,
                    end: b,
                    before,
                    after,
                    kinds,
                });
            }
        }
        let (left, right) = (
            tokens[candidate.query_start].start,
            tokens[candidate.query_end - 1].end,
        );
        let local = splice(span(text, left, right), &edits, left)?;
        let actual = tokenize(&local);
        let expected = &self.canonical[candidate.reference_start..candidate.reference_end];
        let matches = actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(token, word)| nfc(&local[token.start..token.end]) == *word);
        if !matches {
            return Err(Issue); // rendered words must equal the canonical target
        }
        Ok(edits)
    }
}

fn signed(value: usize) -> isize {
    isize::try_from(value).expect("positions fit in isize")
}
