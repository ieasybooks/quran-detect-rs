//! The public detection, planning and replacement API.

use std::collections::{HashMap, HashSet};

use crate::alignment::{Candidate, Interval, OpKind, Search, Settings, external_prefix, select};
use crate::config::{Config, ErrorKind};
use crate::corpus::{Corpus, corpus, text_sha256};
use crate::edition::{Edition, Script};
use crate::error::Error;
use crate::formatting::{CharConflict, Conflict, Formatting, formatting_edits};
use crate::mapping::map_text;
use crate::model::{Edit, Match, ReferenceAlternative, SCORE_METHOD, VerseRef};
use crate::plan::{
    DETECTOR_VERSION, OFFSET_UNIT, PLAN_VERSION, Plan, PlanEdit, Proposal, Status, Summary, sha256,
    splice,
};
use crate::render::{CharEdit, Renderer, span, splice as splice_chars};
use crate::text::{Token, tokenize};

/// Detects Quran quotations and restores their canonical wording.
///
/// A detector is cheap to clone and safe to share between threads. The embedded corpus
/// index is built once per process, on first use.
#[derive(Clone, Debug)]
pub struct Detector {
    config: Config,
    script: Script,
}

/// A rendered candidate accepted for its span.
struct Accepted {
    start: usize,
    end: usize,
    candidate: Candidate,
    edits: Vec<CharEdit>,
}

impl Interval for Accepted {
    fn start(&self) -> usize {
        self.start
    }

    fn end(&self) -> usize {
        self.end
    }

    fn utility(&self) -> i64 {
        let support = i64::try_from(self.candidate.support_count()).expect("small");
        let edits = i64::try_from(self.candidate.edits).expect("small");
        (4 * support - 2 * edits - 8).max(1)
    }
}

/// Byte offsets of each code point, plus the text length.
struct Offsets(Vec<usize>);

impl Offsets {
    fn new(text: &str) -> Self {
        Self(
            text.char_indices()
                .map(|(byte, _)| byte)
                .chain(std::iter::once(text.len()))
                .collect(),
        )
    }

    fn byte(&self, index: usize) -> usize {
        self.0[index]
    }

    /// The code-point index at a byte offset that is known to be a character boundary.
    fn char(&self, byte: usize) -> usize {
        self.0.binary_search(&byte).expect("a character boundary")
    }

    fn edit(&self, edit: CharEdit) -> Edit {
        Edit {
            start: self.byte(edit.start),
            end: self.byte(edit.end),
            before: edit.before,
            after: edit.after,
            kinds: edit.kinds,
        }
    }
}

impl Default for Detector {
    /// The balanced preset, rendering Tanzil Simple Plain.
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl Detector {
    /// A detector for `config`, rendering the default script (Simple Plain).
    pub fn new(config: Config) -> Self {
        Self {
            config,
            script: Script::default(),
        }
    }

    /// Render restored quotations in `script`; detection is the same for every script.
    #[must_use]
    pub fn with_script(self, script: Script) -> Self {
        Self { script, ..self }
    }

    /// The matching configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The rendered script.
    pub fn script(&self) -> Script {
        self.script
    }

    /// The reading, verse count and script that matches refer to.
    pub fn edition(&self) -> Edition {
        Edition::hafs(self.script)
    }

    /// Recommended, nonoverlapping matches in source order.
    pub fn identify(&self, text: &str) -> Vec<Match> {
        self.matches(text, false)
    }

    /// Every executable candidate, including overlapping alternatives, ordered by span.
    ///
    /// The `recommended` subset equals [`Detector::identify`].
    pub fn alternatives(&self, text: &str) -> Vec<Match> {
        self.matches(text, true)
    }

    /// A plan for the recommended matches.
    pub fn plan(&self, text: &str, formatting: Formatting) -> Plan {
        let matches = self.identify(text);
        self.plan_matches(text, &matches, formatting)
            .expect("identified matches are valid for their own input")
    }

    /// A plan for exactly `matches`, which are applied as given and never reranked.
    ///
    /// Matches must come from this input, in order and without overlaps. A match's
    /// `reference_choice` selects one of its `reference_alternatives`.
    pub fn plan_matches(
        &self,
        text: &str,
        matches: &[Match],
        formatting: Formatting,
    ) -> Result<Plan, Error> {
        self.plan_matches_in(corpus(), text, matches, formatting)
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn plan_matches_in(
        &self,
        corpus: &Corpus,
        text: &str,
        matches: &[Match],
        formatting: Formatting,
    ) -> Result<Plan, Error> {
        let chars: Vec<char> = text.chars().collect();
        let offsets = Offsets::new(text);
        let input_hash = sha256(text);
        let mut cursor = 0;
        let mut proposals = Vec::with_capacity(matches.len());
        for (index, m) in matches.iter().enumerate() {
            let index = index + 1;
            if m.input_sha256 != input_hash
                || !(cursor <= m.start && m.start < m.end)
                || text.get(m.start..m.end) != Some(m.text.as_str())
            {
                return Err(Error::StaleMatches);
            }
            if m.edits
                .iter()
                .any(|e| !(m.start <= e.start && e.start <= e.end && e.end <= m.end))
            {
                return Err(Error::EditOutsideMatch(index));
            }
            cursor = m.end;
            if m.edition != self.edition() {
                return Err(Error::EditionMismatch {
                    index,
                    expected: self.edition(),
                    found: m.edition,
                });
            }
            let (reference_start, reference_end, references) =
                self.resolve_reference(corpus, m, index)?;
            let (adjacent, conflicts) = if formatting.is_noop() {
                (Vec::new(), Vec::new())
            } else {
                formatting_edits(
                    &chars,
                    (offsets.char(m.start), offsets.char(m.end)),
                    (reference_start, reference_end),
                    corpus,
                    formatting,
                )
            };
            let attribute = |edit: Edit| PlanEdit {
                start: edit.start,
                end: edit.end,
                before: edit.before,
                after: edit.after,
                kinds: edit.kinds,
                proposal_id: index,
            };
            let lexical: Vec<PlanEdit> = m.edits.iter().cloned().map(attribute).collect();
            let adjacent: Vec<PlanEdit> = adjacent
                .into_iter()
                .map(|edit| attribute(offsets.edit(edit)))
                .collect();
            let conflicts = conflicts
                .into_iter()
                .map(|conflict: CharConflict| Conflict {
                    kind: "reference_conflict".into(),
                    start: offsets.byte(conflict.start),
                    end: offsets.byte(conflict.end),
                    existing: conflict.existing,
                    expected: conflict.expected,
                })
                .collect();
            let mut edits: Vec<PlanEdit> = lexical.iter().chain(&adjacent).cloned().collect();
            edits.sort_by_key(|edit| (edit.start, edit.end));
            proposals.push(Proposal {
                id: index,
                candidate_id: m.id.clone(),
                start: m.start,
                end: m.end,
                text: m.text.clone(),
                before: m.text.clone(),
                canonical_text: m.canonical_text.clone(),
                references,
                reference_start,
                reference_end,
                alignment_errors: m.alignment_errors,
                error_fraction: m.error_fraction,
                equally_ranked_targets: m.equally_ranked_targets,
                input_sha256: m.input_sha256.clone(),
                recommended: m.recommended,
                score: m.score,
                score_method: m.score_method.clone(),
                reference_alternatives: m.reference_alternatives.clone(),
                reference_choice: m.reference_choice.clone(),
                edition: m.edition,
                status: if lexical.is_empty() && adjacent.is_empty() {
                    Status::Unchanged
                } else {
                    Status::Replace
                },
                reason: "accepted_lexical_match".into(),
                lexical_edits: lexical,
                formatting_edits: adjacent,
                formatting_conflicts: conflicts,
                edits,
            });
        }
        let mut edits: Vec<PlanEdit> = proposals
            .iter()
            .flat_map(|p| p.edits.iter().cloned())
            .collect();
        edits.sort_by_key(|edit| (edit.start, edit.end));
        let output = splice(
            text,
            edits
                .iter()
                .map(|e| (e.start, e.end, e.before.as_str(), e.after.as_str())),
        )?;
        let summary = Summary {
            replace: proposals
                .iter()
                .filter(|p| p.status == Status::Replace)
                .count(),
            unchanged: proposals
                .iter()
                .filter(|p| p.status == Status::Unchanged)
                .count(),
        };
        Ok(Plan {
            version: PLAN_VERSION.into(),
            detector_version: DETECTOR_VERSION.into(),
            config: self.config,
            formatting,
            edition: self.edition(),
            offset_unit: OFFSET_UNIT.into(),
            input_sha256: input_hash,
            output_sha256: sha256(&output),
            input_bytes: text.len(),
            output_bytes: output.len(),
            corpus_sha256: text_sha256(self.script).into(),
            proposals,
            edits,
            summary,
        })
    }

    /// The text with recommended matches restored.
    pub fn replace(&self, text: &str, formatting: Formatting) -> String {
        self.plan(text, formatting)
            .apply(text)
            .expect("a plan applies to its own input")
    }

    /// The text with exactly `matches` restored; `[]` leaves it unchanged.
    pub fn replace_matches(
        &self,
        text: &str,
        matches: &[Match],
        formatting: Formatting,
    ) -> Result<String, Error> {
        self.plan_matches(text, matches, formatting)?.apply(text)
    }

    fn search<'c>(&self, corpus: &'c Corpus) -> Search<'c> {
        let allowed = self.config.allowed_errors();
        let context = self.config.min_exact_context();
        Search {
            corpus,
            settings: Settings {
                typos: allowed.contains(ErrorKind::Typo),
                structural: allowed.iter().any(|kind| kind != ErrorKind::Typo),
                allow_space_errors: allowed.contains(ErrorKind::Join)
                    || allowed.contains(ErrorKind::Split),
                error_fraction: self.config.error_fraction(),
                min_words: usize_from(self.config.min_words()).max(3),
                max_edits: usize_from(self.config.max_edits()),
                max_extension: 64,
            },
            allowed,
            min_words: usize_from(self.config.min_words()),
            context: (usize_from(context.before), usize_from(context.after)),
        }
    }

    /// `_prepare`: cut an external prefix, check permissions and render the edits.
    fn prepare(
        search: &Search<'_>,
        renderer: &Renderer<'_>,
        text: &[char],
        tokens: &[Token],
        mut candidate: Candidate,
    ) -> Option<Accepted> {
        let words = &search.corpus.words;
        let first = candidate.operations[0];
        let mut cut_tokens: Option<Vec<Token>> = None;
        if first.query_end - first.query_start == 1
            && first.reference_end - first.reference_start == 1
        {
            let token = &tokens[candidate.query_start];
            let canonical = &words[candidate.reference_start];
            if let Some(prefix) = external_prefix(&token.norm, canonical) {
                let mapped = map_text(&text[token.start..token.end]);
                if mapped.token_count() != 1 {
                    return None;
                }
                let cut = mapped.cut_first(prefix.chars().count()).ok()?;
                if !mapped.legal_cut(cut) {
                    return None;
                }
                let mut copy = tokens.to_vec();
                copy[candidate.query_start] = Token {
                    norm: canonical.clone(),
                    start: token.start + cut,
                    end: token.end,
                    block: token.block,
                };
                let delta = usize::from(first.kind != OpKind::Exact);
                candidate.operations[0].kind = OpKind::Exact;
                candidate.edits -= delta;
                candidate.score += 2 * i64::try_from(delta).expect("zero or one");
                cut_tokens = Some(copy);
            }
        }
        let tokens = cut_tokens.as_deref().unwrap_or(tokens);
        if candidate.operations.iter().any(|op| {
            op.kind
                .error()
                .is_some_and(|error| !search.allowed.contains(error))
        }) {
            return None;
        }
        let edits = renderer.candidate_edits(text, tokens, &candidate).ok()?;
        Some(Accepted {
            start: tokens[candidate.query_start].start,
            end: tokens[candidate.query_end - 1].end,
            candidate,
            edits,
        })
    }

    fn matches(&self, text: &str, alternatives: bool) -> Vec<Match> {
        self.matches_in(corpus(), text, alternatives)
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn matches_in(&self, corpus: &Corpus, text: &str, alternatives: bool) -> Vec<Match> {
        let chars: Vec<char> = text.chars().collect();
        let tokens = tokenize(&chars);
        if tokens.is_empty() {
            return Vec::new();
        }
        let search = self.search(corpus);
        let canonical = corpus.canonical(self.script);
        let renderer = Renderer { corpus, canonical };
        let mut groups: Vec<((usize, usize), Vec<Accepted>)> = Vec::new();
        let mut group_of: HashMap<(usize, usize), usize> = HashMap::new();
        for candidate in search.candidates(&tokens) {
            if let Some(accepted) = Self::prepare(&search, &renderer, &chars, &tokens, candidate) {
                let span = (accepted.start, accepted.end);
                let at = *group_of.entry(span).or_insert_with(|| {
                    groups.push((span, Vec::new()));
                    groups.len() - 1
                });
                groups[at].1.push(accepted);
            }
        }
        for (_, group) in &mut groups {
            group.sort_by(|a, b| {
                let key = |m: &Accepted| {
                    let c = &m.candidate;
                    (c.edits, -c.score, c.reference_start, c.reference_end)
                };
                key(a).cmp(&key(b))
            });
        }
        let firsts: Vec<&Accepted> = groups.iter().map(|(_, group)| &group[0]).collect();
        let selected: Vec<usize> = select(&firsts);
        let selected_spans: HashSet<(usize, usize)> =
            selected.iter().map(|&i| groups[i].0).collect();
        let order: Vec<usize> = if alternatives {
            let mut all: Vec<usize> = (0..groups.len()).collect();
            all.sort_by_key(|&i| groups[i].0);
            all
        } else {
            selected
        };
        let offsets = Offsets::new(text);
        let input_hash = sha256(text);
        let edition = self.edition();
        let mut results = Vec::new();
        for at in order {
            let (span, group) = &groups[at];
            let mut outcomes: Vec<(String, Vec<usize>)> = Vec::new();
            for (member, m) in group.iter().enumerate() {
                let output: String = splice_chars(span_chars(&chars, m), &m.edits, m.start)
                    .expect("edits were validated against this span")
                    .into_iter()
                    .collect();
                match outcomes.iter_mut().find(|(seen, _)| *seen == output) {
                    Some((_, members)) => members.push(member),
                    None => outcomes.push((output, vec![member])),
                }
            }
            for (rendered, members) in outcomes {
                let recommended = selected_spans.contains(span) && members[0] == 0;
                if !alternatives && !recommended {
                    continue;
                }
                let m = &group[members[0]];
                let c = &m.candidate;
                let same: HashSet<(usize, usize)> = group
                    .iter()
                    .map(|p| &p.candidate)
                    .filter(|p| (p.edits, p.score) == (c.edits, c.score))
                    .map(|p| (p.reference_start, p.reference_end))
                    .collect();
                let mut seen = HashSet::new();
                let reference_alternatives = members
                    .iter()
                    .map(|&member| &group[member].candidate)
                    .filter(|target| seen.insert((target.reference_start, target.reference_end)))
                    .map(|target| ReferenceAlternative {
                        id: reference_id(target.reference_start, target.reference_end),
                        reference_start: target.reference_start,
                        reference_end: target.reference_end,
                        references: verse_refs(
                            corpus,
                            target.reference_start,
                            target.reference_end,
                        ),
                    })
                    .collect();
                let (start, end) = (offsets.byte(m.start), offsets.byte(m.end));
                let query_words = c.query_end - c.query_start;
                let reference_words = c.reference_end - c.reference_start;
                #[allow(clippy::cast_precision_loss, reason = "word counts are far below 2^52")]
                let error_fraction = c.edits as f64 / query_words.max(reference_words) as f64;
                results.push(Match {
                    start,
                    end,
                    text: text[start..end].to_owned(),
                    canonical_text: canonical[c.reference_start..c.reference_end].join(" "),
                    references: verse_refs(corpus, c.reference_start, c.reference_end),
                    reference_start: c.reference_start,
                    reference_end: c.reference_end,
                    alignment_errors: c.edits,
                    error_fraction,
                    equally_ranked_targets: same.len(),
                    edits: m
                        .edits
                        .iter()
                        .cloned()
                        .map(|edit| offsets.edit(edit))
                        .collect(),
                    input_sha256: input_hash.clone(),
                    recommended,
                    score: m.utility(),
                    score_method: SCORE_METHOD.into(),
                    reference_alternatives,
                    edition,
                    id: sha256(&format!("{input_hash}:{start}:{end}:{rendered}")),
                    reference_choice: None,
                });
            }
        }
        results
    }

    /// The selected occurrence, honoring a valid `reference_choice`.
    fn resolve_reference(
        &self,
        corpus: &Corpus,
        m: &Match,
        index: usize,
    ) -> Result<(usize, usize, Vec<VerseRef>), Error> {
        let words = corpus.words.len();
        let Some(choice) = &m.reference_choice else {
            if !(m.reference_start < m.reference_end && m.reference_end <= words) {
                return Err(Error::InvalidReferenceCoordinates(index));
            }
            return Ok((m.reference_start, m.reference_end, m.references.clone()));
        };
        let mut choices = m.reference_alternatives.iter().filter(|r| r.id == *choice);
        let (Some(chosen), None) = (choices.next(), choices.next()) else {
            return Err(Error::UnknownReferenceChoice(index));
        };
        let (start, end) = (chosen.reference_start, chosen.reference_end);
        if !(start < end && end <= words) || chosen.id != reference_id(start, end) {
            return Err(Error::InvalidReferenceCoordinates(index));
        }
        let canonical = corpus.canonical(self.script)[start..end].join(" ");
        let references = verse_refs(corpus, start, end);
        let connected = (start..end - 1).all(|p| corpus.connected(signed(p), signed(p + 1)));
        if canonical != m.canonical_text || references != chosen.references || !connected {
            return Err(Error::ReferenceMismatch(index));
        }
        Ok((start, end, references))
    }
}

fn span_chars<'a>(chars: &'a [char], m: &Accepted) -> &'a [char] {
    span(chars, m.start, m.end)
}

fn reference_id(start: usize, end: usize) -> String {
    format!("{start}:{end}")
}

fn verse_refs(corpus: &Corpus, start: usize, end: usize) -> Vec<VerseRef> {
    corpus
        .references(start, end)
        .into_iter()
        .map(|(surah, verse)| VerseRef(surah, verse))
        .collect()
}

fn usize_from(value: u32) -> usize {
    usize::try_from(value).expect("u32 fits in usize")
}

fn signed(value: usize) -> isize {
    isize::try_from(value).expect("positions fit in isize")
}
