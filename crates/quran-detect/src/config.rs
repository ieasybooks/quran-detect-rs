//! Matching configuration: presets, error budgets and allowed alignment errors.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A non-exact alignment operation that matching may allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorKind {
    /// Remove an interior input word.
    Extra,
    /// Separate two canonical words merged into one input word.
    Join,
    /// Insert an interior canonical word.
    Missing,
    /// Join a canonical word split across two input words.
    Split,
    /// Replace an unrelated interior word directly.
    Substitution,
    /// One normalized character inserted, deleted or replaced; one word has three letters.
    Typo,
}

impl ErrorKind {
    /// Every error kind, in name order.
    pub const ALL: [Self; 6] = [
        Self::Extra,
        Self::Join,
        Self::Missing,
        Self::Split,
        Self::Substitution,
        Self::Typo,
    ];

    /// The stable name used by the command line and JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::Extra => "extra",
            Self::Join => "join",
            Self::Missing => "missing",
            Self::Split => "split",
            Self::Substitution => "substitution",
            Self::Typo => "typo",
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for ErrorKind {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == value)
            .ok_or_else(|| ConfigError::UnknownErrorKind(value.to_owned()))
    }
}

/// A set of allowed error kinds.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ErrorKinds(u8);

impl ErrorKinds {
    /// No errors: exact normalized matches only.
    pub const NONE: Self = Self(0);

    /// Whether `kind` is allowed.
    pub fn contains(self, kind: ErrorKind) -> bool {
        self.0 & kind.bit() != 0
    }

    /// This set with `kind` added.
    #[must_use]
    pub fn with(self, kind: ErrorKind) -> Self {
        Self(self.0 | kind.bit())
    }

    /// Whether no error is allowed.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The allowed kinds, in name order.
    pub fn iter(self) -> impl Iterator<Item = ErrorKind> {
        ErrorKind::ALL
            .into_iter()
            .filter(move |kind| self.contains(*kind))
    }

    /// Parse a comma-separated list such as `typo,missing`; an empty string allows none.
    pub fn parse_list(list: &str) -> Result<Self, ConfigError> {
        list.split(',')
            .filter(|name| !name.is_empty())
            .try_fold(Self::NONE, |kinds, name| Ok(kinds.with(name.parse()?)))
    }
}

impl FromIterator<ErrorKind> for ErrorKinds {
    fn from_iter<I: IntoIterator<Item = ErrorKind>>(iter: I) -> Self {
        iter.into_iter().fold(Self::NONE, Self::with)
    }
}

impl fmt::Debug for ErrorKinds {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.iter()).finish()
    }
}

impl Serialize for ErrorKinds {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for ErrorKinds {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Vec::<ErrorKind>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

/// Exact words required before and after a word-changing gap, in one verse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ExactContext {
    /// Consecutive exact words required before the gap.
    pub before: u32,
    /// Consecutive exact words required after the gap.
    pub after: u32,
}

impl ExactContext {
    /// The same requirement on both sides; zero disables the check.
    pub fn both(words: u32) -> Self {
        Self {
            before: words,
            after: words,
        }
    }

    /// Parse `N` or `BEFORE,AFTER`, as accepted by `--min-exact-context`.
    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        let number = |part: &str| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(ConfigError::ExactContext(value.to_owned()));
            }
            part.parse::<u32>()
                .map_err(|_| ConfigError::ExactContext(value.to_owned()))
        };
        match value.split_once(',') {
            None => Ok(Self::both(number(value)?)),
            Some((before, after)) => Ok(Self {
                before: number(before)?,
                after: number(after)?,
            }),
        }
    }
}

impl Serialize for ExactContext {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        [self.before, self.after].serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExactContext {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [before, after] = <[u32; 2]>::deserialize(deserializer)?;
        Ok(Self { before, after })
    }
}

/// A named aggressiveness level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// Exact normalized matches only.
    Strict,
    /// Typo, missing, extra, join and split errors within a 25% budget.
    #[default]
    Balanced,
    /// Balanced errors plus direct substitution within a 35% budget.
    Tolerant,
}

impl Preset {
    /// Every preset.
    pub const ALL: [Self; 3] = [Self::Strict, Self::Balanced, Self::Tolerant];

    /// The stable name used by the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Balanced => "balanced",
            Self::Tolerant => "tolerant",
        }
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for Preset {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.name() == value)
            .ok_or_else(|| ConfigError::UnknownPreset(value.to_owned()))
    }
}

/// An invalid configuration value.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The error fraction is not finite or not between 0 and 1.
    #[error("error fraction must be finite and between 0 and 1")]
    ErrorFraction,
    /// The minimum canonical word count is below two.
    #[error("minimum words must be at least 2")]
    MinWords,
    /// An error kind name was not recognized.
    #[error("unknown error kind {0:?}; expected typo, missing, extra, join, split or substitution")]
    UnknownErrorKind(String),
    /// A preset name was not recognized.
    #[error("unknown preset {0:?}; expected strict, balanced or tolerant")]
    UnknownPreset(String),
    /// An exact-context value was neither `N` nor `BEFORE,AFTER`.
    #[error("exact context {0:?} must be a nonnegative integer or BEFORE,AFTER pair")]
    ExactContext(String),
}

/// How aggressively the detector matches damaged quotations.
///
/// Error budgets are edit limits, not confidence probabilities. Both the fraction and
/// the absolute limit must pass; the fraction's denominator is the larger of the input
/// and canonical word counts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawConfig")]
pub struct Config {
    error_fraction: f64,
    max_edits: u32,
    min_words: u32,
    allowed_errors: ErrorKinds,
    min_exact_context: ExactContext,
}

/// Deserialized values, validated before they become a [`Config`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    error_fraction: f64,
    max_edits: u32,
    min_words: u32,
    allowed_errors: ErrorKinds,
    min_exact_context: ExactContext,
}

impl TryFrom<RawConfig> for Config {
    type Error = ConfigError;

    fn try_from(raw: RawConfig) -> Result<Self, Self::Error> {
        Self::default()
            .with_error_fraction(raw.error_fraction)?
            .with_min_words(raw.min_words)
            .map(|config| {
                config
                    .with_max_edits(raw.max_edits)
                    .with_allowed_errors(raw.allowed_errors)
                    .with_min_exact_context(raw.min_exact_context)
            })
    }
}

impl Default for Config {
    /// The balanced preset.
    fn default() -> Self {
        Self::preset(Preset::Balanced)
    }
}

impl Config {
    /// The settings of a named preset; every preset requires two exact context words.
    pub fn preset(preset: Preset) -> Self {
        let base = Self {
            error_fraction: 0.25,
            max_edits: 8,
            min_words: 3,
            allowed_errors: [
                ErrorKind::Typo,
                ErrorKind::Missing,
                ErrorKind::Extra,
                ErrorKind::Join,
                ErrorKind::Split,
            ]
            .into_iter()
            .collect(),
            min_exact_context: ExactContext::both(2),
        };
        match preset {
            Preset::Strict => Self {
                error_fraction: 0.0,
                max_edits: 0,
                allowed_errors: ErrorKinds::NONE,
                ..base
            },
            Preset::Balanced => base,
            Preset::Tolerant => Self {
                error_fraction: 0.35,
                allowed_errors: ErrorKind::ALL.into_iter().collect(),
                ..base
            },
        }
    }

    /// Maximum alignment errors as a fraction of the passage length, from 0 to 1.
    pub fn with_error_fraction(self, error_fraction: f64) -> Result<Self, ConfigError> {
        if !error_fraction.is_finite() || !(0.0..=1.0).contains(&error_fraction) {
            return Err(ConfigError::ErrorFraction);
        }
        Ok(Self {
            error_fraction,
            ..self
        })
    }

    /// Maximum alignment errors in one match.
    #[must_use]
    pub fn with_max_edits(self, max_edits: u32) -> Self {
        Self { max_edits, ..self }
    }

    /// Minimum canonical words in a match; at least two.
    pub fn with_min_words(self, min_words: u32) -> Result<Self, ConfigError> {
        if min_words < 2 {
            return Err(ConfigError::MinWords);
        }
        Ok(Self { min_words, ..self })
    }

    /// The alignment errors matching may use.
    #[must_use]
    pub fn with_allowed_errors(self, allowed_errors: ErrorKinds) -> Self {
        Self {
            allowed_errors,
            ..self
        }
    }

    /// Exact words required around groups containing missing, extra or substituted words.
    #[must_use]
    pub fn with_min_exact_context(self, min_exact_context: ExactContext) -> Self {
        Self {
            min_exact_context,
            ..self
        }
    }

    /// Maximum alignment errors as a fraction of the passage length.
    pub fn error_fraction(&self) -> f64 {
        self.error_fraction
    }

    /// Maximum alignment errors in one match.
    pub fn max_edits(&self) -> u32 {
        self.max_edits
    }

    /// Minimum canonical words in a match.
    pub fn min_words(&self) -> u32 {
        self.min_words
    }

    /// The alignment errors matching may use.
    pub fn allowed_errors(&self) -> ErrorKinds {
        self.allowed_errors
    }

    /// Exact words required around word-changing gaps.
    pub fn min_exact_context(&self) -> ExactContext {
        self.min_exact_context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_match_the_reference_definitions() {
        let strict = Config::preset(Preset::Strict);
        assert_eq!(strict.error_fraction(), 0.0);
        assert_eq!(strict.max_edits(), 0);
        assert!(strict.allowed_errors().is_empty());
        let balanced = Config::default();
        assert_eq!(balanced, Config::preset(Preset::Balanced));
        assert_eq!(
            balanced.allowed_errors().iter().collect::<Vec<_>>(),
            [
                ErrorKind::Extra,
                ErrorKind::Join,
                ErrorKind::Missing,
                ErrorKind::Split,
                ErrorKind::Typo
            ]
        );
        let tolerant = Config::preset(Preset::Tolerant);
        assert_eq!(tolerant.error_fraction(), 0.35);
        assert_eq!(tolerant.allowed_errors().iter().count(), 6);
        for config in [strict, balanced, tolerant] {
            assert_eq!(config.min_words(), 3);
            assert_eq!(config.min_exact_context(), ExactContext::both(2));
        }
    }

    #[test]
    fn values_are_validated_and_parsed_like_the_reference_cli() {
        let config = Config::default();
        assert!(config.with_error_fraction(f64::NAN).is_err());
        assert!(config.with_error_fraction(1.5).is_err());
        assert!(config.with_error_fraction(1.0).is_ok());
        assert!(config.with_min_words(1).is_err());
        assert!(config.with_min_words(2).is_ok());
        assert_eq!(ExactContext::parse("0"), Ok(ExactContext::both(0)));
        assert_eq!(
            ExactContext::parse("1,2"),
            Ok(ExactContext {
                before: 1,
                after: 2
            })
        );
        for invalid in ["", "-1", "1,", "1,2,3", "a", " 1", "1.5"] {
            assert!(ExactContext::parse(invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(ErrorKinds::parse_list(""), Ok(ErrorKinds::NONE));
        assert_eq!(
            ErrorKinds::parse_list("typo,,join")
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [ErrorKind::Join, ErrorKind::Typo]
        );
        assert!(ErrorKinds::parse_list("typo,bad").is_err());
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert_eq!(
            json,
            r#"{"error_fraction":0.25,"max_edits":8,"min_words":3,"allowed_errors":["extra","join","missing","split","typo"],"min_exact_context":[2,2]}"#
        );
        assert_eq!(
            serde_json::from_str::<Config>(&json).unwrap(),
            Config::default()
        );
        let invalid = json.replace("0.25", "1.5");
        assert!(serde_json::from_str::<Config>(&invalid).is_err());
        let invalid = json.replace("\"min_words\":3", "\"min_words\":1");
        assert!(serde_json::from_str::<Config>(&invalid).is_err());
    }
}
