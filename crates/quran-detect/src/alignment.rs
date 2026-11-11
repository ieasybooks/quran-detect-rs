//! Positional seeding, bounded alignment and candidate validation.
//!
//! This is a line-by-line port of the reference matcher. Positions are word indices:
//! `query` positions index the input tokens and `reference` positions index corpus words.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::config::{ErrorKind, ErrorKinds};
use crate::corpus::Corpus;
use crate::text::Token;

/// Prefixes that may stay outside a match while the rest of the word starts it.
pub(crate) const PREFIXES: [&str; 11] =
    ["و", "ف", "ب", "ك", "ل", "وب", "فب", "ول", "فل", "وك", "فك"];

/// Alignment operations, declared in the alphabetical order of their names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum OpKind {
    Exact,
    Extra,
    Join,
    Missing,
    Split,
    Substitution,
    Typo,
}

impl OpKind {
    /// Operations whose input words support the match.
    fn supports(self) -> bool {
        matches!(self, Self::Exact | Self::Typo | Self::Join | Self::Split)
    }

    /// The configurable error kind, or `None` for an exact word.
    pub(crate) fn error(self) -> Option<ErrorKind> {
        match self {
            Self::Exact => None,
            Self::Extra => Some(ErrorKind::Extra),
            Self::Join => Some(ErrorKind::Join),
            Self::Missing => Some(ErrorKind::Missing),
            Self::Split => Some(ErrorKind::Split),
            Self::Substitution => Some(ErrorKind::Substitution),
            Self::Typo => Some(ErrorKind::Typo),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Operation {
    pub(crate) kind: OpKind,
    pub(crate) query_start: usize,
    pub(crate) query_end: usize,
    pub(crate) reference_start: usize,
    pub(crate) reference_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) query_start: usize,
    pub(crate) query_end: usize,
    pub(crate) reference_start: usize,
    pub(crate) reference_end: usize,
    pub(crate) edits: usize,
    pub(crate) score: i64,
    pub(crate) operations: Vec<Operation>,
}

impl Candidate {
    /// Input words aligned by exact, typo, join or split operations.
    pub(crate) fn support_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|op| op.kind.supports())
            .map(|op| op.query_end - op.query_start)
            .sum()
    }
}

#[derive(Clone, Debug, Default)]
struct Path {
    edits: usize,
    score: i64,
    operations: Vec<Operation>,
}

/// A shared traceback avoids copying an entire path at every transition.
struct Trace {
    edits: usize,
    score: i64,
    previous: Option<usize>,
    operation: Option<Operation>,
}

/// Search settings derived from the public configuration.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    pub(crate) typos: bool,
    pub(crate) structural: bool,
    pub(crate) allow_space_errors: bool,
    pub(crate) error_fraction: f64,
    /// At least three: shortening and whole-verse rescue use this, not the configured minimum.
    pub(crate) min_words: usize,
    pub(crate) max_edits: usize,
    pub(crate) max_extension: usize,
}

/// The configuration a search needs besides its settings.
pub(crate) struct Search<'c> {
    pub(crate) corpus: &'c Corpus,
    pub(crate) settings: Settings,
    pub(crate) allowed: ErrorKinds,
    /// The configured minimum canonical words, which may be two.
    pub(crate) min_words: usize,
    pub(crate) context: (usize, usize),
}

type Seed = (usize, usize, usize, usize, OpKind);

/// `external_prefix`: a supported prefix that leaves exactly `canonical` behind.
pub(crate) fn external_prefix<'a>(observed: &'a str, canonical: &str) -> Option<&'a str> {
    if observed == canonical {
        return None;
    }
    let prefix = observed.strip_suffix(canonical)?;
    PREFIXES.contains(&prefix).then_some(prefix)
}

/// Whether the Levenshtein distance between `a` and `b` is exactly one.
pub(crate) fn distance_is_one(a: &str, b: &str) -> bool {
    let (a_len, b_len) = (a.chars().count(), b.chars().count());
    let (short, long) = if a_len <= b_len { (a, b) } else { (b, a) };
    match a_len.abs_diff(b_len) {
        0 => {
            short
                .chars()
                .zip(long.chars())
                .filter(|(x, y)| x != y)
                .count()
                == 1
        }
        1 => {
            // Skip the common prefix; the rest must match after one extra long character.
            let mut short = short.chars().peekable();
            let mut long = long.chars();
            while let Some(&x) = short.peek() {
                let y = long.next().expect("the longer word has more characters");
                if x != y {
                    return short.eq(long);
                }
                short.next();
            }
            true
        }
        _ => false,
    }
}

/// Whether `whole` is `left` followed by `right`, without allocating.
fn concatenates(whole: &str, left: &str, right: &str) -> bool {
    whole.len() == left.len() + right.len() && whole.starts_with(left) && whole.ends_with(right)
}

fn typo_pair(a: &str, b: &str) -> bool {
    a.chars().count().max(b.chars().count()) >= 3 && distance_is_one(a, b)
}

/// One side's endpoints, keyed by edit count in first-insertion order.
type Endpoints = Vec<(usize, (usize, usize, Path))>;

/// Endpoints during the search: edit count -> (i, j, trace index or the empty path).
type TraceEndpoints = Vec<(usize, (usize, usize, Option<usize>))>;

/// An unreached state in the dense alignment table.
const NONE: usize = usize::MAX;

fn upsert(endpoints: &mut Endpoints, edits: usize, value: (usize, usize, Path)) {
    if let Some(slot) = endpoints.iter_mut().find(|(key, _)| *key == edits) {
        slot.1 = value;
    } else {
        endpoints.push((edits, value));
    }
}

impl Search<'_> {
    fn query_connected(tokens: &[Token], a: isize, b: isize) -> bool {
        match (usize::try_from(a), usize::try_from(b)) {
            (Ok(a), Ok(b)) if a < tokens.len() && b < tokens.len() => {
                tokens[a].block == tokens[b].block
            }
            _ => false,
        }
    }

    fn connected_query(tokens: &[Token], a: usize, b: usize) -> bool {
        a < tokens.len() && b < tokens.len() && tokens[a].block == tokens[b].block
    }

    pub(crate) fn seeds(&self, tokens: &[Token]) -> BTreeSet<Seed> {
        let c = self.corpus;
        let mut seeds = BTreeSet::new();
        let mut covered: HashSet<(usize, usize)> = HashSet::new();
        for i in 0..tokens.len().saturating_sub(1) {
            if !Self::connected_query(tokens, i, i + 1) {
                continue;
            }
            for &p in c.bigrams(&tokens[i].norm, &tokens[i + 1].norm) {
                if covered.contains(&(i, p)) {
                    continue;
                }
                let (mut left, mut ref_left, mut right, mut ref_right) = (i, p, i + 2, p + 2);
                while left > 0
                    && ref_left > 0
                    && Self::connected_query(tokens, left - 1, left)
                    && c.connected(signed(ref_left) - 1, signed(ref_left))
                    && tokens[left - 1].norm == c.words[ref_left - 1]
                {
                    left -= 1;
                    ref_left -= 1;
                }
                while right < tokens.len()
                    && ref_right < c.words.len()
                    && Self::connected_query(tokens, right - 1, right)
                    && c.connected(signed(ref_right) - 1, signed(ref_right))
                    && tokens[right].norm == c.words[ref_right]
                {
                    right += 1;
                    ref_right += 1;
                }
                seeds.insert((left, right, ref_left, ref_right, OpKind::Exact));
                covered.extend((left..right).map(|q| (q, ref_left + q - left)));
            }
        }
        let s = &self.settings;
        if s.structural && s.allow_space_errors {
            // Anchor words with one lost or extra space at explicit canonical positions.
            for (i, token) in tokens.iter().enumerate() {
                for &p in c.joined(&token.norm) {
                    seeds.insert((i, i + 1, p, p + 2, OpKind::Join));
                }
                if i + 1 < tokens.len() && Self::connected_query(tokens, i, i + 1) {
                    let joined = format!("{}{}", token.norm, tokens[i + 1].norm);
                    for &p in c.positions(&joined) {
                        seeds.insert((i, i + 2, p, p + 1, OpKind::Split));
                    }
                }
            }
        }
        if s.typos || s.structural {
            // Rare exact words rescue short quotations without an intact bigram.
            for (i, token) in tokens.iter().enumerate() {
                let positions = c.positions(&token.norm);
                if token.norm.chars().count() >= 3
                    && (1..=32).contains(&positions.len())
                    && !c.is_stopword(&token.norm)
                {
                    for &p in positions {
                        if !covered.contains(&(i, p)) {
                            seeds.insert((i, i + 1, p, p + 1, OpKind::Exact));
                        }
                    }
                }
            }
        }
        // A prefix cut supplies an exact first word, including in strict mode. It is
        // valid only at the final candidate's left boundary.
        for q in 0..tokens.len().saturating_sub(1) {
            if !Self::connected_query(tokens, q, q + 1) {
                continue;
            }
            for n in [1, 2] {
                let norm = &tokens[q].norm;
                let split = norm
                    .char_indices()
                    .nth(n)
                    .map_or(norm.len(), |(index, _)| index);
                let (prefix, core) = norm.split_at(split);
                if !PREFIXES.contains(&prefix) || core.is_empty() {
                    continue;
                }
                for &r in c.bigrams(core, &tokens[q + 1].norm) {
                    let (mut end, mut reference_end) = (q + 2, r + 2);
                    while end < tokens.len()
                        && reference_end < c.words.len()
                        && Self::connected_query(tokens, end - 1, end)
                        && c.connected(signed(reference_end) - 1, signed(reference_end))
                        && tokens[end].norm == c.words[reference_end]
                    {
                        end += 1;
                        reference_end += 1;
                    }
                    seeds.insert((q, end, r, reference_end, OpKind::Exact));
                }
            }
        }
        seeds
    }

    /// The best path for each edit count while extending one side of a seed.
    fn side(
        &self,
        tokens: &[Token],
        query_cursor: isize,
        reference_cursor: isize,
        direction: isize,
        max_edits: usize,
    ) -> Vec<(usize, usize, Path)> {
        let c = self.corpus;
        let s = &self.settings;
        let mut qs: Vec<usize> = Vec::new();
        for d in 0..signed(s.max_extension) {
            let q = query_cursor + direction * d;
            let Ok(index) = usize::try_from(q) else { break };
            if index >= tokens.len() {
                break;
            }
            let neighbor = q - direction;
            if usize::try_from(neighbor).is_ok_and(|n| n < tokens.len())
                && !Self::query_connected(tokens, q, neighbor)
            {
                break;
            }
            qs.push(index);
        }
        let mut rs: Vec<usize> = Vec::new();
        for d in 0..signed(s.max_extension) {
            let r = reference_cursor + direction * d;
            let Ok(index) = usize::try_from(r) else { break };
            if index >= c.words.len() || !c.connected(r, r - direction) {
                break;
            }
            rs.push(index);
        }
        if qs.is_empty() || rs.is_empty() {
            return vec![(0, 0, Path::default())];
        }
        if !s.structural {
            return self.simple_side(tokens, &qs, &rs, max_edits);
        }
        self.aligned_side(
            tokens,
            &qs,
            &rs,
            query_cursor,
            reference_cursor,
            direction,
            max_edits,
        )
    }

    /// Word-by-word exact and typo extension when no structural edit is allowed.
    fn simple_side(
        &self,
        tokens: &[Token],
        qs: &[usize],
        rs: &[usize],
        max_edits: usize,
    ) -> Vec<(usize, usize, Path)> {
        let c = self.corpus;
        let (mut edits, mut score) = (0_usize, 0_i64);
        let mut operations = Vec::new();
        let mut saved: Vec<(usize, (usize, i64))> = Vec::new();
        for (i, (&q, &p)) in qs.iter().zip(rs).enumerate() {
            let (a, b) = (&tokens[q].norm, &c.words[p]);
            let (kind, cost, reward) = if a == b {
                (OpKind::Exact, 0, 4)
            } else if self.settings.typos && typo_pair(a, b) {
                (OpKind::Typo, 1, 2)
            } else {
                break;
            };
            if edits + cost > max_edits {
                break;
            }
            operations.push(Operation {
                kind,
                query_start: q,
                query_end: q + 1,
                reference_start: p,
                reference_end: p + 1,
            });
            edits += cost;
            score += reward;
            let value = (i + 1, score);
            if let Some(slot) = saved.iter_mut().find(|(key, _)| *key == edits) {
                slot.1 = value;
            } else {
                saved.push((edits, value));
            }
        }
        let mut endpoints: Endpoints = vec![(0, (0, 0, Path::default()))];
        for (edits, (length, score)) in saved {
            let path = Path {
                edits,
                score,
                operations: operations[..length].to_vec(),
            };
            upsert(&mut endpoints, edits, (length, length, path));
        }
        endpoints.into_iter().map(|(_, value)| value).collect()
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn aligned_side(
        &self,
        tokens: &[Token],
        qs: &[usize],
        rs: &[usize],
        query_cursor: isize,
        reference_cursor: isize,
        direction: isize,
        max_edits: usize,
    ) -> Vec<(usize, usize, Path)> {
        let c = self.corpus;
        let interval =
            |indices: &[usize], at: usize, count: usize, cursor: isize| -> (usize, usize) {
                if count > 0 {
                    let selected = &indices[at..at + count];
                    let low = *selected.iter().min().expect("nonempty");
                    let high = *selected.iter().max().expect("nonempty");
                    (low, high + 1)
                } else {
                    let boundary = cursor + direction * signed(at) + isize::from(direction < 0);
                    let boundary = usize::try_from(boundary).expect("boundary inside the input");
                    (boundary, boundary)
                }
            };
        let mut arena = vec![Trace {
            edits: 0,
            score: 0,
            previous: None,
            operation: None,
        }];
        let (max_q, max_r) = (qs.len(), rs.len());
        // A dense (i, j) table of trace indices; `NONE` marks an unreached state.
        let width = max_r + 1;
        let mut states = vec![NONE; (max_q + 1) * width];
        states[0] = 0;
        let mut active = vec![false; max_q + 1];
        active[0] = true;
        // Edit count -> (i, j, trace index); `None` is the initial empty path.
        let mut endpoints: TraceEndpoints = vec![(0, (0, 0, None))];

        let step = |arena: &mut Vec<Trace>,
                    states: &mut Vec<usize>,
                    active: &mut Vec<bool>,
                    (i, j): (usize, usize),
                    (a, b): (usize, usize),
                    kind: OpKind,
                    cost: usize,
                    reward: i64| {
            if kind
                .error()
                .is_some_and(|error| !self.allowed.contains(error))
            {
                return;
            }
            let from = states[i * width + j];
            let (edits, score) = (arena[from].edits + cost, arena[from].score + reward);
            if edits > max_edits {
                return;
            }
            let key = (i + a) * width + j + b;
            let old = states[key];
            if old != NONE && (edits, -score) >= (arena[old].edits, -arena[old].score) {
                return;
            }
            let (query_start, query_end) = interval(qs, i, a, query_cursor);
            let (reference_start, reference_end) = interval(rs, j, b, reference_cursor);
            arena.push(Trace {
                edits,
                score,
                previous: Some(from),
                operation: Some(Operation {
                    kind,
                    query_start,
                    query_end,
                    reference_start,
                    reference_end,
                }),
            });
            states[key] = arena.len() - 1;
            active[i + a] = true;
        };

        for i in 0..=max_q {
            if !active[i] {
                continue;
            }
            let low = i.saturating_sub(max_edits);
            let high = max_r.min(i.saturating_add(max_edits));
            for j in low..=high {
                let current = states[i * width + j];
                if current == NONE {
                    continue;
                }
                let trace = &arena[current];
                if trace.operation.is_some_and(|op| op.kind.supports()) {
                    let better = match endpoints.iter().find(|(key, _)| *key == trace.edits) {
                        None => true,
                        Some((_, (oi, oj, old))) => {
                            let old_score = old.map_or(0, |old| arena[old].score);
                            (trace.score, i + j) > (old_score, oi + oj)
                        }
                    };
                    if better {
                        let edits = trace.edits;
                        if let Some(slot) = endpoints.iter_mut().find(|(key, _)| *key == edits) {
                            slot.1 = (i, j, Some(current));
                        } else {
                            endpoints.push((edits, (i, j, Some(current))));
                        }
                    }
                }
                if i < max_q && j < max_r {
                    let (a, b) = (&tokens[qs[i]].norm, &c.words[rs[j]]);
                    if a == b {
                        step(
                            &mut arena,
                            &mut states,
                            &mut active,
                            (i, j),
                            (1, 1),
                            OpKind::Exact,
                            0,
                            4,
                        );
                    } else if self.settings.typos && typo_pair(a, b) {
                        step(
                            &mut arena,
                            &mut states,
                            &mut active,
                            (i, j),
                            (1, 1),
                            OpKind::Typo,
                            1,
                            2,
                        );
                    } else {
                        step(
                            &mut arena,
                            &mut states,
                            &mut active,
                            (i, j),
                            (1, 1),
                            OpKind::Substitution,
                            1,
                            -3,
                        );
                    }
                }
                if i < max_q {
                    step(
                        &mut arena,
                        &mut states,
                        &mut active,
                        (i, j),
                        (1, 0),
                        OpKind::Extra,
                        1,
                        -3,
                    );
                }
                if j < max_r {
                    step(
                        &mut arena,
                        &mut states,
                        &mut active,
                        (i, j),
                        (0, 1),
                        OpKind::Missing,
                        1,
                        -3,
                    );
                }
                if i + 1 < max_q && j < max_r {
                    let (x, y) = (qs[i].min(qs[i + 1]), qs[i].max(qs[i + 1]));
                    if concatenates(&c.words[rs[j]], &tokens[x].norm, &tokens[y].norm) {
                        step(
                            &mut arena,
                            &mut states,
                            &mut active,
                            (i, j),
                            (2, 1),
                            OpKind::Split,
                            1,
                            2,
                        );
                    }
                }
                if i < max_q && j + 1 < max_r {
                    let (x, y) = (rs[j].min(rs[j + 1]), rs[j].max(rs[j + 1]));
                    if concatenates(&tokens[qs[i]].norm, &c.words[x], &c.words[y]) {
                        step(
                            &mut arena,
                            &mut states,
                            &mut active,
                            (i, j),
                            (1, 2),
                            OpKind::Join,
                            1,
                            2,
                        );
                    }
                }
            }
        }
        endpoints
            .into_iter()
            .map(|(_, (i, j, trace))| {
                (
                    i,
                    j,
                    trace.map_or_else(Path::default, |trace| path(&arena, trace)),
                )
            })
            .collect()
    }

    /// `_valid`: candidate shape, error budget, atomic boundaries, prefixes and context.
    pub(crate) fn valid(&self, c: &Candidate, tokens: &[Token]) -> bool {
        let query_len = c.query_end - c.query_start;
        let reference_len = c.reference_end - c.reference_start;
        if reference_len < self.min_words || c.support_count() < 2 {
            return false;
        }
        #[allow(clippy::cast_precision_loss, reason = "word counts are far below 2^52")]
        let budget = self.settings.error_fraction * query_len.max(reference_len) as f64;
        #[allow(clippy::cast_precision_loss, reason = "edit counts are far below 2^52")]
        if c.edits > self.settings.max_edits || c.edits as f64 > budget {
            return false;
        }
        if c.query_start > 0 && tokens[c.query_start - 1].end > tokens[c.query_start].start {
            return false;
        }
        if c.query_end < tokens.len() && tokens[c.query_end].start < tokens[c.query_end - 1].end {
            return false;
        }
        for op in &c.operations {
            if op.kind == OpKind::Exact
                && tokens[op.query_start].norm != self.corpus.words[op.reference_start]
                && (op.query_start != c.query_start
                    || external_prefix(
                        &tokens[op.query_start].norm,
                        &self.corpus.words[op.reference_start],
                    )
                    .is_none())
            {
                return false;
            }
        }
        // Word-changing gaps need local evidence: a long quotation must not lend its edit
        // budget to surrounding prose or a neighboring verse.
        self.context_valid(c)
    }

    fn context_valid(&self, c: &Candidate) -> bool {
        let (before, after) = self.context;
        if before == 0 && after == 0 {
            return true;
        }
        let operations = &c.operations;
        let mut i = 0;
        while i < operations.len() {
            if operations[i].kind == OpKind::Exact {
                i += 1;
                continue;
            }
            let mut end = i + 1;
            while end < operations.len() && operations[end].kind != OpKind::Exact {
                end += 1;
            }
            if operations[i..end].iter().any(|op| {
                matches!(
                    op.kind,
                    OpKind::Missing | OpKind::Extra | OpKind::Substitution
                )
            }) {
                if i < before || end + after > operations.len() {
                    return false;
                }
                let anchors: Vec<&Operation> = operations[i - before..i]
                    .iter()
                    .chain(&operations[end..end + after])
                    .collect();
                if anchors.iter().any(|op| op.kind != OpKind::Exact) {
                    return false;
                }
                // Include consumed core words when one side needs no anchors; for an
                // extra-only run, the anchors alone determine its verse.
                let owner = self.corpus.owner[anchors[0].reference_start];
                let supported = anchors.iter().copied().chain(
                    operations[i..end]
                        .iter()
                        .filter(|op| op.reference_end > op.reference_start),
                );
                for op in supported {
                    if self.corpus.owner[op.reference_start] != owner
                        || self.corpus.owner[op.reference_end - 1] != owner
                    {
                        return false;
                    }
                }
            }
            i = end;
        }
        true
    }

    /// `_candidates`: every valid candidate, keyed by span, best first-insertion order.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn candidates(&self, tokens: &[Token]) -> Vec<Candidate> {
        let s = self.settings;
        let mut seeds = self.seeds(tokens);
        let mut blocks: HashMap<usize, (usize, usize)> = HashMap::new();
        for (i, token) in tokens.iter().enumerate() {
            blocks
                .entry(token.block)
                .and_modify(|span| span.1 = i + 1)
                .or_insert((i, i + 1));
        }
        let exact = |q0: usize, q1: usize, r0: usize, r1: usize| Candidate {
            query_start: q0,
            query_end: q1,
            reference_start: r0,
            reference_end: r1,
            edits: 0,
            score: 4 * signed64(q1 - q0),
            operations: exact_operations(q0, q1, r0),
        };
        // Only an accepted whole-block match can safely bypass fuzzy work. A rejected
        // maximal seed must not hide a legal shorter prefix or a complete verse inside it.
        let mut exact_blocks: HashSet<usize> = HashSet::new();
        let mut additional: BTreeSet<Seed> = BTreeSet::new();
        for &(q0, q1, r0, r1, kind) in &seeds {
            if kind != OpKind::Exact {
                continue;
            }
            if self.valid(&exact(q0, q1, r0, r1), tokens) {
                if blocks[&tokens[q0].block] == (q0, q1) {
                    exact_blocks.insert(tokens[q0].block);
                }
                continue;
            }
            let mut end = r1 - 1;
            while end - r0 >= s.min_words {
                let query_end = q0 + end - r0;
                if self.valid(&exact(q0, query_end, r0, end), tokens) {
                    additional.insert((q0, query_end, r0, end, OpKind::Exact));
                    break;
                }
                // At six words the short-fragment rule no longer applies, so shortening
                // a rejected long prefix cannot help. Whole verses are handled below.
                if end - r0 >= 6 {
                    break;
                }
                end -= 1;
            }
            for owner in self.corpus.owner[r0]..=self.corpus.owner[r1 - 1] {
                let verse = &self.corpus.verses[owner];
                let (start, end) = (verse.offset, verse.end());
                if r0 <= start && end <= r1 && verse.len >= s.min_words {
                    additional.insert((q0 + start - r0, q0 + end - r0, start, end, OpKind::Exact));
                }
            }
        }
        seeds.extend(additional);
        let mut limits: HashMap<usize, usize> = HashMap::new();
        for (&block, &(start, end)) in &blocks {
            // Every supported operation has rlen <= qlen + edits, so edits <= f * (block
            // words + edits). This cap cannot discard a candidate within the error fraction.
            let mut cap = s.max_edits;
            if s.error_fraction < 1.0 {
                #[allow(clippy::cast_precision_loss, reason = "word counts are far below 2^52")]
                let words = (end - start) as f64;
                let bound = (s.error_fraction * words / (1.0 - s.error_fraction) + 1e-12).floor();
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a small nonnegative integer"
                )]
                let bound = bound as usize;
                cap = cap.min(bound);
            }
            limits.insert(block, cap);
        }
        let mut accepted: Vec<Candidate> = Vec::new();
        let mut index: HashMap<(usize, usize, usize, usize), usize> = HashMap::new();
        for &(q0, q1, r0, r1, kind) in &seeds {
            let block = tokens[q0].block;
            let complete_exact = exact_blocks.contains(&block);
            if complete_exact && (kind != OpKind::Exact || blocks[&block] != (q0, q1)) {
                continue;
            }
            let seed = if kind == OpKind::Exact {
                Path {
                    edits: 0,
                    score: 4 * signed64(q1 - q0),
                    operations: exact_operations(q0, q1, r0),
                }
            } else {
                Path {
                    edits: 1,
                    score: 2,
                    operations: vec![Operation {
                        kind,
                        query_start: q0,
                        query_end: q1,
                        reference_start: r0,
                        reference_end: r1,
                    }],
                }
            };
            let limit = limits[&block];
            if seed.edits > limit {
                continue;
            }
            let (left, right) = if (s.typos || s.structural) && !complete_exact {
                let budget = limit - seed.edits;
                (
                    self.side(tokens, signed(q0) - 1, signed(r0) - 1, -1, budget),
                    self.side(tokens, signed(q1), signed(r1), 1, budget),
                )
            } else {
                (vec![(0, 0, Path::default())], vec![(0, 0, Path::default())])
            };
            for (ql, rl, lpath) in &left {
                for (qr, rr, rpath) in &right {
                    let edits = seed.edits + lpath.edits + rpath.edits;
                    if edits > limit {
                        continue;
                    }
                    let mut operations: Vec<Operation> =
                        lpath.operations.iter().rev().copied().collect();
                    operations.extend_from_slice(&seed.operations);
                    operations.extend_from_slice(&rpath.operations);
                    let candidate = Candidate {
                        query_start: q0 - ql,
                        query_end: q1 + qr,
                        reference_start: r0 - rl,
                        reference_end: r1 + rr,
                        edits,
                        score: seed.score + lpath.score + rpath.score,
                        operations,
                    };
                    if !self.valid(&candidate, tokens) {
                        continue;
                    }
                    let key = (
                        candidate.query_start,
                        candidate.query_end,
                        candidate.reference_start,
                        candidate.reference_end,
                    );
                    match index.get(&key) {
                        None => {
                            index.insert(key, accepted.len());
                            accepted.push(candidate);
                        }
                        Some(&at) => {
                            let old = &accepted[at];
                            if (candidate.edits, -candidate.score) < (old.edits, -old.score) {
                                accepted[at] = candidate;
                            }
                        }
                    }
                }
            }
        }
        accepted
    }
}

fn path(arena: &[Trace], mut current: usize) -> Path {
    let (edits, score) = (arena[current].edits, arena[current].score);
    let mut operations = Vec::new();
    while let Some(operation) = arena[current].operation {
        operations.push(operation);
        current = arena[current]
            .previous
            .expect("every operation has a predecessor");
    }
    operations.reverse();
    Path {
        edits,
        score,
        operations,
    }
}

fn exact_operations(q0: usize, q1: usize, r0: usize) -> Vec<Operation> {
    (q0..q1)
        .map(|q| Operation {
            kind: OpKind::Exact,
            query_start: q,
            query_end: q + 1,
            reference_start: r0 + q - q0,
            reference_end: r0 + q - q0 + 1,
        })
        .collect()
}

fn signed(value: usize) -> isize {
    isize::try_from(value).expect("positions fit in isize")
}

fn signed64(value: usize) -> i64 {
    i64::try_from(value).expect("scores fit in i64")
}

/// A span chosen for selection, with its interval utility.
pub(crate) trait Interval {
    fn start(&self) -> usize;
    fn end(&self) -> usize;
    fn utility(&self) -> i64;
}

impl<T: Interval> Interval for &T {
    fn start(&self) -> usize {
        (*self).start()
    }
    fn end(&self) -> usize {
        (*self).end()
    }
    fn utility(&self) -> i64 {
        (*self).utility()
    }
}

/// Maximum-weight nonoverlapping intervals with deterministic ties.
///
/// Returns indices into `matches`, ordered by `(start, end)`.
pub(crate) fn select<T: Interval>(matches: &[T]) -> Vec<usize> {
    let mut ordered: Vec<usize> = (0..matches.len()).collect();
    ordered.sort_by_key(|&i| (matches[i].end(), matches[i].start()));
    let ends: Vec<usize> = ordered.iter().map(|&i| matches[i].end()).collect();
    let mut dp: Vec<(i64, i64, i64)> = vec![(0, 0, 0)];
    let mut predecessors = vec![0_usize];
    let mut taken = vec![false];
    for (i, &m) in ordered.iter().enumerate() {
        let m = &matches[m];
        let previous = ends[..i].partition_point(|&end| end <= m.start());
        let base = dp[previous];
        let take = (
            base.0 + m.utility(),
            base.1 - 1,
            base.2 + signed64(m.end() - m.start()),
        );
        let skip = *dp.last().expect("nonempty");
        // On identical objectives keep the earlier result.
        let used = take > skip;
        dp.push(if used { take } else { skip });
        predecessors.push(if used { previous } else { i });
        taken.push(used);
    }
    let mut chosen = Vec::new();
    let mut cursor = ordered.len();
    while cursor > 0 {
        if taken[cursor] {
            chosen.push(ordered[cursor - 1]);
        }
        cursor = predecessors[cursor];
    }
    chosen.sort_by_key(|&i| (matches[i].start(), matches[i].end()));
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levenshtein(a: &[char], b: &[char]) -> usize {
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, x) in a.iter().enumerate() {
            let mut diagonal = row[0];
            row[0] = i + 1;
            for (j, y) in b.iter().enumerate() {
                let above = row[j + 1];
                row[j + 1] = (above + 1)
                    .min(row[j] + 1)
                    .min(diagonal + usize::from(x != y));
                diagonal = above;
            }
        }
        row[b.len()]
    }

    #[test]
    fn distance_one_agrees_with_levenshtein_on_every_short_string() {
        let alphabet = ['ا', 'ب', 'ت'];
        let mut words = vec![String::new()];
        for length in 1..=4 {
            let mut next = Vec::new();
            for word in words.iter().filter(|w| w.chars().count() == length - 1) {
                for c in alphabet {
                    next.push(format!("{word}{c}"));
                }
            }
            words.extend(next);
        }
        for a in &words {
            for b in &words {
                let expected = levenshtein(
                    &a.chars().collect::<Vec<_>>(),
                    &b.chars().collect::<Vec<_>>(),
                ) == 1;
                assert_eq!(distance_is_one(a, b), expected, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn prefixes_must_leave_the_whole_canonical_word() {
        assert_eq!(external_prefix("وقل", "قل"), Some("و"));
        assert_eq!(external_prefix("فبما", "ما"), Some("فب"));
        assert_eq!(external_prefix("قل", "قل"), None);
        assert_eq!(external_prefix("مقل", "قل"), None);
        assert_eq!(external_prefix("وقل", "ل"), None);
    }

    struct Span(usize, usize, i64);

    impl Interval for Span {
        fn start(&self) -> usize {
            self.0
        }
        fn end(&self) -> usize {
            self.1
        }
        fn utility(&self) -> i64 {
            self.2
        }
    }

    #[test]
    fn selection_maximizes_utility_then_prefers_fewer_longer_spans() {
        let spans = [
            Span(0, 10, 5),
            Span(0, 4, 3),
            Span(5, 10, 3),
            Span(12, 14, 1),
        ];
        assert_eq!(select(&spans), [1, 2, 3]);
        let tie = [Span(0, 4, 2), Span(4, 8, 2), Span(0, 8, 4)];
        assert_eq!(select(&tie), [2]);
        assert_eq!(select::<Span>(&[]), Vec::<usize>::new());
    }
}
