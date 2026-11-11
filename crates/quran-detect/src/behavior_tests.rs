//! Behavioral contracts on small synthetic corpora, ported from the reference test suite.

use crate::config::{Config, ErrorKind, ErrorKinds, ExactContext, Preset};
use crate::corpus::Corpus;
use crate::detector::Detector;
use crate::formatting::Formatting;
use crate::model::Match;

const CANONICAL: &str = "عظيم كريم رحيم لطيف خبير حكيم";

fn only(kind: ErrorKind) -> ErrorKinds {
    ErrorKinds::NONE.with(kind)
}

fn identify(corpus: &Corpus, config: Config, text: &str) -> Vec<Match> {
    Detector::new(config).matches_in(corpus, text, false)
}

/// The restored text, after checking that its plan reverts exactly.
fn replace(corpus: &Corpus, config: Config, text: &str) -> String {
    let detector = Detector::new(config);
    let matches = detector.matches_in(corpus, text, false);
    let plan = detector
        .plan_matches_in(corpus, text, &matches, Formatting::default())
        .unwrap();
    let output = plan.apply(text).unwrap();
    assert_eq!(plan.revert(&output).unwrap(), text);
    output
}

#[test]
fn the_absolute_budget_is_separate_from_the_fraction() {
    let corpus = Corpus::from_verses(&[(1, 1, CANONICAL)]);
    let text = "عظيم كريمم رحيم لطيف خبيير حكيم";
    let config = Config::default()
        .with_error_fraction(0.5)
        .unwrap()
        .with_max_edits(1)
        .with_allowed_errors(only(ErrorKind::Typo));
    assert_ne!(replace(&corpus, config, text), CANONICAL);
    assert_eq!(replace(&corpus, config.with_max_edits(2), text), CANONICAL);
}

#[test]
fn allowed_error_kinds_are_independent() {
    let corpus = Corpus::from_verses(&[(1, 1, CANONICAL)]);
    for (kind, source) in [
        (ErrorKind::Extra, "عظيم كريم رحيم زيادة لطيف خبير حكيم"),
        (ErrorKind::Missing, "عظيم كريم لطيف خبير حكيم"),
        (ErrorKind::Substitution, "عظيم كريم غريب لطيف خبير حكيم"),
        (ErrorKind::Join, "عظيم كريم رحيملطيف خبير حكيم"),
        (ErrorKind::Split, "عظيم كريم رح يم لطيف خبير حكيم"),
    ] {
        let config = Config::default().with_allowed_errors(only(kind));
        let matches = identify(&corpus, config, source);
        assert!(
            matches
                .iter()
                .any(|m| m.start == 0 && m.end == source.len()),
            "{kind}"
        );
        assert_eq!(replace(&corpus, config, source), CANONICAL, "{kind}");
        let none = Config::default().with_allowed_errors(ErrorKinds::NONE);
        assert_eq!(replace(&corpus, none, source), source, "{kind}");
    }
}

#[test]
fn one_boundary_word_cannot_authorize_deleting_prose() {
    let corpus = Corpus::from_verses(&[(1, 1, CANONICAL)]);
    for source in [
        "عظيم زيادة كريم رحيم لطيف خبير حكيم",
        "عظيم كريم رحيم لطيف خبير زيادة حكيم",
    ] {
        let config = Config::default().with_allowed_errors(only(ErrorKind::Extra));
        assert!(replace(&corpus, config, source).contains("زيادة"));
        let permissive = config.with_min_exact_context(ExactContext::both(1));
        assert_eq!(replace(&corpus, permissive, source), CANONICAL);
    }
}

#[test]
fn a_structural_gap_cannot_borrow_context_from_a_neighboring_verse() {
    let corpus = Corpus::from_verses(&[(1, 1, "عظيم كريم رحيم نذير"), (1, 2, "لطيف خبير حكيم")]);
    let source = CANONICAL;
    let guarded = Config::default().with_allowed_errors(only(ErrorKind::Missing));
    assert_eq!(replace(&corpus, guarded, source), source);
    let legacy = guarded.with_min_exact_context(ExactContext::both(0));
    assert_eq!(
        replace(&corpus, legacy, source),
        "عظيم كريم رحيم نذير لطيف خبير حكيم"
    );
}

#[test]
fn context_minima_are_independent_per_side() {
    let corpus = Corpus::from_verses(&[(1, 1, CANONICAL)]);
    let side = |before, after| {
        Config::default()
            .with_allowed_errors(only(ErrorKind::Extra))
            .with_min_exact_context(ExactContext { before, after })
    };
    for (source, accepted, rejected) in [
        (
            "عظيم زيادة كريم رحيم لطيف خبير حكيم",
            side(1, 2),
            side(2, 1),
        ),
        (
            "عظيم كريم رحيم لطيف خبير زيادة حكيم",
            side(2, 1),
            side(1, 2),
        ),
    ] {
        assert_eq!(replace(&corpus, accepted, source), CANONICAL);
        assert!(replace(&corpus, rejected, source).contains("زيادة"));
    }
}

#[test]
fn one_zero_side_still_guards_verse_boundaries() {
    let corpus = Corpus::from_verses(&[(1, 1, "عظيم كريم رحيم"), (1, 2, "لطيف خبير حكيم")]);
    let source = "عظيم كريم رحيم زيادة لطيف خبير حكيم";
    for (before, after) in [(0, 2), (2, 0)] {
        // For an extra-only gap, the required side determines the owner verse.
        let config = Config::default()
            .with_allowed_errors(only(ErrorKind::Extra))
            .with_min_exact_context(ExactContext { before, after });
        assert_eq!(replace(&corpus, config, source), CANONICAL);
        let both = config.with_min_exact_context(ExactContext::both(2));
        assert!(replace(&corpus, both, source).contains("زيادة"));
    }
}

#[test]
fn distinct_canonical_spellings_are_separate_alternatives() {
    let corpus =
        Corpus::from_verses(&[(1, 1, "عَظِيم كريم رحيم لطيف"), (2, 1, "عظيم كريم رحيم لطيف")]);
    let detector = Detector::new(Config::preset(Preset::Strict));
    let candidates = detector.matches_in(&corpus, "عظيم كريم رحيم لطيف", true);
    assert_eq!(candidates.len(), 2);
    assert_ne!(candidates[0].id, candidates[1].id);
    assert!(
        candidates
            .iter()
            .all(|c| c.reference_alternatives.len() == 1)
    );
    assert_eq!(candidates.iter().filter(|c| c.recommended).count(), 1);
}
