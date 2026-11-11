//! Saved replacement plans with hash-checked application and exact undo.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::edition::Edition;
use crate::error::Error;
use crate::formatting::{Conflict, Formatting};
use crate::model::{EditKind, ReferenceAlternative, VerseRef};

/// The plan format written by this version; plans store UTF-8 byte offsets.
pub const PLAN_VERSION: &str = "2.0";
/// The matching algorithm recorded in plans.
pub const DETECTOR_VERSION: &str = "lexical-matcher-2";
/// How plan offsets are measured.
pub const OFFSET_UNIT: &str = "UTF-8 bytes, [start,end)";

/// An edit in a plan, attributed to one proposal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanEdit {
    /// First byte of the replaced text.
    pub start: usize,
    /// Byte after the replaced text.
    pub end: usize,
    /// The text currently at `start..end`.
    pub before: String,
    /// The replacement text.
    pub after: String,
    /// The operations that produced this edit, in name order.
    pub kinds: Vec<EditKind>,
    /// The one-based proposal this edit belongs to.
    pub proposal_id: usize,
}

/// Whether a proposal changes the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// At least one edit.
    Replace,
    /// The quotation is already canonical and no formatting applies.
    Unchanged,
}

/// One accepted match with its lexical and formatting edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// One-based position in the plan.
    pub id: usize,
    /// The match's identity.
    pub candidate_id: String,
    /// First byte of the matched input text.
    pub start: usize,
    /// Byte after the matched input text.
    pub end: usize,
    /// The matched input text.
    pub text: String,
    /// The matched input text before replacement; equal to `text`.
    pub before: String,
    /// The canonical wording that replaces it.
    pub canonical_text: String,
    /// The verses of the selected canonical occurrence.
    pub references: Vec<VerseRef>,
    /// First canonical word position.
    pub reference_start: usize,
    /// Canonical word position after the occurrence.
    pub reference_end: usize,
    /// Non-exact alignment operations used.
    pub alignment_errors: usize,
    /// `alignment_errors` over the larger word count.
    pub error_fraction: f64,
    /// Distinct canonical occurrences tied with the selected one.
    pub equally_ranked_targets: usize,
    /// SHA-256 of the whole input.
    pub input_sha256: String,
    /// Whether automatic selection chose the match.
    pub recommended: bool,
    /// Deterministic interval utility, not a probability.
    pub score: i64,
    /// How `score` is defined.
    pub score_method: String,
    /// Occurrences that render the match identically.
    pub reference_alternatives: Vec<ReferenceAlternative>,
    /// The chosen alternative, if one was chosen.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::model::present"
    )]
    pub reference_choice: Option<String>,
    /// The canonical text the match refers to.
    pub edition: Edition,
    /// Whether the proposal changes the text.
    pub status: Status,
    /// Always `accepted_lexical_match`.
    pub reason: String,
    /// Edits that restore the quotation.
    pub lexical_edits: Vec<PlanEdit>,
    /// Bracket and citation edits next to the quotation.
    pub formatting_edits: Vec<PlanEdit>,
    /// Existing citations that differ from the library's.
    pub formatting_conflicts: Vec<Conflict>,
    /// Lexical and formatting edits in source order.
    pub edits: Vec<PlanEdit>,
}

/// Proposal counts by status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    /// Proposals that change the text.
    pub replace: usize,
    /// Proposals that leave the text unchanged.
    pub unchanged: usize,
}

/// A JSON-serializable record of every edit, for audit trails and exact undo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// [`PLAN_VERSION`].
    pub version: String,
    /// [`DETECTOR_VERSION`].
    pub detector_version: String,
    /// The configuration that produced the matches.
    pub config: Config,
    /// The formatting options applied.
    pub formatting: Formatting,
    /// The canonical text the matches refer to.
    pub edition: Edition,
    /// [`OFFSET_UNIT`].
    pub offset_unit: String,
    /// SHA-256 of the input.
    pub input_sha256: String,
    /// SHA-256 of the output.
    pub output_sha256: String,
    /// Input length in bytes.
    pub input_bytes: usize,
    /// Output length in bytes.
    pub output_bytes: usize,
    /// SHA-256 of the embedded Tanzil text that was rendered.
    pub corpus_sha256: String,
    /// Accepted matches in source order.
    pub proposals: Vec<Proposal>,
    /// Every edit in source order.
    pub edits: Vec<PlanEdit>,
    /// Proposal counts by status.
    pub summary: Summary,
}

impl Plan {
    /// Apply the plan to its exact original input; offsets are never rebased.
    pub fn apply(&self, input: &str) -> Result<String, Error> {
        self.check_version()?;
        if sha256(input) != self.input_sha256 {
            return Err(Error::InputHash);
        }
        let output = splice(
            input,
            self.edits
                .iter()
                .map(|e| (e.start, e.end, e.before.as_str(), e.after.as_str())),
        )?;
        if sha256(&output) != self.output_sha256 {
            return Err(Error::OutputHash);
        }
        Ok(output)
    }

    /// Reverse exactly this plan, restoring the original bytes.
    pub fn revert(&self, output: &str) -> Result<String, Error> {
        self.check_version()?;
        if sha256(output) != self.output_sha256 {
            return Err(Error::OutputHash);
        }
        let mut inverse = Vec::with_capacity(self.edits.len());
        let mut delta: isize = 0;
        for edit in &self.edits {
            let start = edit
                .start
                .checked_add_signed(delta)
                .ok_or(Error::InvalidEdit)?;
            let end = start
                .checked_add(edit.after.len())
                .ok_or(Error::InvalidEdit)?;
            inverse.push((start, end, edit.after.as_str(), edit.before.as_str()));
            // A hand-edited plan can hold any offsets: reject what cannot be a real edit.
            let removed = edit.end.checked_sub(edit.start).ok_or(Error::InvalidEdit)?;
            let (Ok(added), Ok(removed)) =
                (isize::try_from(edit.after.len()), isize::try_from(removed))
            else {
                return Err(Error::InvalidEdit);
            };
            delta = delta
                .checked_add(added)
                .and_then(|delta| delta.checked_sub(removed))
                .ok_or(Error::InvalidEdit)?;
        }
        let input = splice(output, inverse)?;
        if sha256(&input) != self.input_sha256 {
            return Err(Error::InputHash);
        }
        Ok(input)
    }

    fn check_version(&self) -> Result<(), Error> {
        if self.version == PLAN_VERSION {
            Ok(())
        } else {
            Err(Error::PlanVersion(self.version.clone()))
        }
    }
}

/// Lowercase hexadecimal SHA-256 of UTF-8 text.
pub(crate) fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
            hex
        })
}

/// Apply ordered, nonoverlapping `(start, end, before, after)` byte edits.
pub(crate) fn splice<'a>(
    text: &str,
    edits: impl IntoIterator<Item = (usize, usize, &'a str, &'a str)>,
) -> Result<String, Error> {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end, before, after) in edits {
        if !(cursor <= start && start <= end) || text.get(start..end) != Some(before) {
            return Err(Error::InvalidEdit);
        }
        out.push_str(&text[cursor..start]);
        out.push_str(after);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_rejects_stale_overlapping_and_split_characters() {
        let text = "قال هو";
        assert_eq!(splice(text, [(0, 6, "قال", "قل")]), Ok("قل هو".into()));
        assert_eq!(splice(text, [(0, 6, "قول", "قل")]), Err(Error::InvalidEdit));
        assert_eq!(splice(text, [(1, 6, "قال", "")]), Err(Error::InvalidEdit));
        assert_eq!(
            splice(text, [(0, 2, "ق", ""), (0, 2, "ق", "")]),
            Err(Error::InvalidEdit)
        );
        assert_eq!(splice(text, [(0, 99, "", "")]), Err(Error::InvalidEdit));
        assert_eq!(splice(text, [(11, 11, "", "!")]), Ok("قال هو!".into()));
    }

    #[test]
    fn revert_rejects_impossible_hand_edited_offsets() {
        let text = "Zxxxxxx";
        let mut plan = crate::Detector::default().plan(text, crate::Formatting::default());
        plan.output_sha256.clone_from(&plan.input_sha256);
        let edit = |start, end| PlanEdit {
            start,
            end,
            before: String::new(),
            after: String::new(),
            kinds: Vec::new(),
            proposal_id: 1,
        };
        plan.edits = vec![edit(0, usize::MAX), edit(usize::MAX / 2, usize::MAX)];
        assert_eq!(plan.revert(text), Err(Error::InvalidEdit));
        assert_eq!(plan.apply(text), Err(Error::InvalidEdit));
        plan.edits = vec![edit(3, 2)];
        assert_eq!(plan.revert(text), Err(Error::InvalidEdit));
        assert_eq!(plan.apply(text), Err(Error::InvalidEdit));
        plan.edits = vec![edit(2, 2)];
        assert_eq!(plan.revert(text), Ok(text.to_owned()));
    }

    #[test]
    fn digest_is_lowercase_hex_of_utf8() {
        assert_eq!(
            sha256("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
