//! Detect Quran quotations in Arabic text and restore their canonical wording.
//!
//! The detector matches text against an embedded, verbatim copy of the Tanzil Quran
//! text (Hafs, Kufan verse count). It tolerates configurable damage such as typos and
//! missing or extra words, and rewrites each accepted quotation with the canonical
//! wording. Text outside the quotations is preserved byte for byte.
//!
//! ```
//! use quran_detect::{Config, Detector, Formatting, Preset};
//!
//! let detector = Detector::new(Config::preset(Preset::Strict));
//! let text = "فيختم بقل هو الله أحد";
//! let matches = detector.identify(text);
//! assert_eq!(matches[0].references[0].surah(), 112);
//! // The attached preposition stays outside the quotation.
//! assert_eq!(&text[matches[0].start..matches[0].end], "قل هو الله أحد");
//! assert_eq!(
//!     detector.replace(text, Formatting::default()),
//!     "فيختم بقُلْ هُوَ اللَّهُ أَحَدٌ"
//! );
//! ```
//!
//! Offsets are UTF-8 byte offsets into the input, as half-open `start..end` ranges.
//! Matching does not establish that a writer intended to quote; see the crate README.

// Compile and run the README examples as documentation tests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct Readme;

mod alignment;
#[cfg(test)]
mod behavior_tests;
mod config;
mod corpus;
mod detector;
mod edition;
mod error;
#[cfg(test)]
mod fixture_tests;
mod formatting;
mod html;
mod mapping;
mod model;
mod plan;
mod render;
mod tables;
mod text;
mod unicode;

pub use config::{Config, ConfigError, ErrorKind, ErrorKinds, ExactContext, Preset};
pub use detector::Detector;
pub use edition::{Counting, Edition, Riwaya, Script, UnknownScript};
pub use error::Error;
pub use formatting::{Brackets, Conflict, Formatting};
pub use model::{Edit, EditKind, Match, ReferenceAlternative, SCORE_METHOD, VerseRef};
pub use plan::{
    DETECTOR_VERSION, OFFSET_UNIT, PLAN_VERSION, Plan, PlanEdit, Proposal, Status, Summary,
};
