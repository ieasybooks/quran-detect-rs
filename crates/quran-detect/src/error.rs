//! Errors from applying matches and plans.

use crate::edition::Edition;

/// A match, edit or plan that cannot be applied to the given text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Matches were unordered, overlapping, off character boundaries or from other text.
    #[error("matches must be ordered, nonoverlapping, and from this unchanged input")]
    StaleMatches,
    /// A match contained an edit outside its own span.
    #[error("match {0} has an edit outside its span")]
    EditOutsideMatch(usize),
    /// A match refers to a different reading, count or script than the detector.
    #[error("match {index} is for {found}, but the detector uses {expected}")]
    EditionMismatch {
        /// One-based position of the match.
        index: usize,
        /// The detector's edition.
        expected: Edition,
        /// The match's edition.
        found: Edition,
    },
    /// `reference_choice` named zero or several alternatives.
    #[error("match {0} has an unknown or ambiguous reference_choice")]
    UnknownReferenceChoice(usize),
    /// A chosen alternative has invalid coordinates.
    #[error("match {0} chooses an alternative with invalid reference coordinates")]
    InvalidReferenceCoordinates(usize),
    /// A chosen alternative would change the canonical output or verse path.
    #[error("match {0} chooses a reference that changes the canonical output or path")]
    ReferenceMismatch(usize),
    /// Edits were out of order, overlapping, off character boundaries or stale.
    #[error("invalid, overlapping, or stale edit")]
    InvalidEdit,
    /// The plan format is not supported.
    #[error("unsupported plan version {0:?}; expected {expected:?}", expected = crate::plan::PLAN_VERSION)]
    PlanVersion(String),
    /// The text is not the input this plan was made for.
    #[error("the input does not match the plan's input hash")]
    InputHash,
    /// The text is not the output this plan produces.
    #[error("the text does not match the plan's output hash")]
    OutputHash,
}
