//! Serializable matches and edits. All offsets are UTF-8 byte offsets, half-open.

use serde::{Deserialize, Serialize};

use crate::edition::Edition;

/// How matches are ranked; this is not a probability.
pub const SCORE_METHOD: &str = "legacy_span_priority_then_interval_utility";

/// A `(surah, verse)` reference, serialized as `[surah, verse]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VerseRef(pub u16, pub u16);

impl VerseRef {
    /// The surah number, from 1 to 114.
    pub fn surah(self) -> u16 {
        self.0
    }

    /// The verse number in the edition's counting system.
    pub fn verse(self) -> u16 {
        self.1
    }
}

/// Why an edit exists, in name order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditKind {
    /// A bracket pair converted to braces.
    Bracket,
    /// A word restored to its canonical spelling or vocalization.
    Exact,
    /// An extra input word removed.
    Extra,
    /// A merged input word separated.
    Join,
    /// A missing canonical word inserted.
    Missing,
    /// A citation appended.
    Reference,
    /// A split input word joined.
    Split,
    /// An unrelated word replaced.
    Substitution,
    /// A one-character error corrected.
    Typo,
}

/// Replace `before` at `start..end` with `after`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    /// First byte of the replaced text.
    pub start: usize,
    /// Byte after the replaced text; equal to `start` for an insertion.
    pub end: usize,
    /// The text currently at `start..end`.
    pub before: String,
    /// The replacement text.
    pub after: String,
    /// The operations that produced this edit, in name order.
    pub kinds: Vec<EditKind>,
}

/// One canonical occurrence that renders a match identically.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceAlternative {
    /// `"{reference_start}:{reference_end}"`; pass it as a match's `reference_choice`.
    pub id: String,
    /// First canonical word position.
    pub reference_start: usize,
    /// Canonical word position after the occurrence.
    pub reference_end: usize,
    /// The verses the occurrence covers.
    pub references: Vec<VerseRef>,
}

/// A detected quotation and the edits that restore it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    /// First byte of the matched input text.
    pub start: usize,
    /// Byte after the matched input text.
    pub end: usize,
    /// The matched input text, `input[start..end]`.
    pub text: String,
    /// The canonical wording that replaces it.
    pub canonical_text: String,
    /// The verses of the selected canonical occurrence.
    pub references: Vec<VerseRef>,
    /// First canonical word position of the selected occurrence.
    pub reference_start: usize,
    /// Canonical word position after the selected occurrence.
    pub reference_end: usize,
    /// Non-exact alignment operations used.
    pub alignment_errors: usize,
    /// `alignment_errors` over the larger of the input and canonical word counts.
    pub error_fraction: f64,
    /// Distinct canonical occurrences tied with the selected one; ties use corpus order.
    pub equally_ranked_targets: usize,
    /// Edits inside `start..end`, in order.
    pub edits: Vec<Edit>,
    /// SHA-256 of the whole input; matches only apply to that exact text.
    pub input_sha256: String,
    /// Whether automatic selection chose this match.
    pub recommended: bool,
    /// Deterministic interval utility, not a probability.
    pub score: i64,
    /// How `score` is defined: [`SCORE_METHOD`].
    pub score_method: String,
    /// Occurrences that render this match identically.
    pub reference_alternatives: Vec<ReferenceAlternative>,
    /// The canonical text this match refers to.
    pub edition: Edition,
    /// Stable identity from the input hash, span and rendered text.
    pub id: String,
    /// Optionally set to one of `reference_alternatives[].id` before planning.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub reference_choice: Option<String>,
}

/// Read a present optional field as `Some`, rejecting an explicit `null` instead of
/// treating it as absent, so a hand-edited choice is never silently ignored.
pub(crate) fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::Match;
    use crate::{Detector, Formatting};

    #[test]
    fn an_explicit_null_reference_choice_is_rejected() {
        let detector = Detector::default();
        let matched = detector.identify("قل هو الله أحد").remove(0);
        let mut json = serde_json::to_value(&matched).unwrap();
        assert!(json.get("reference_choice").is_none());
        assert_eq!(
            serde_json::from_value::<Match>(json.clone()).unwrap(),
            matched
        );
        json["reference_choice"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<Match>(json.clone()).is_err());
        let id = matched.reference_alternatives[0].id.clone();
        json["reference_choice"] = serde_json::Value::String(id.clone());
        let chosen: Match = serde_json::from_value(json).unwrap();
        assert_eq!(chosen.reference_choice, Some(id));
        let plan = detector
            .plan_matches("قل هو الله أحد", &[chosen], Formatting::default())
            .unwrap();
        let mut json = serde_json::to_value(&plan).unwrap();
        json["proposals"][0]["reference_choice"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<crate::Plan>(json).is_err());
    }
}
