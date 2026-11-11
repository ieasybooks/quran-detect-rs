//! The canonical text a detector matches and renders.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A transmitted reading (riwaya) of the Quran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Riwaya {
    /// Hafs from Asim, as printed in the Medina Mushaf.
    #[default]
    Hafs,
}

/// A verse-counting system; it determines verse numbers in references.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Counting {
    /// The Kufan count of 6,236 verses, used with Hafs.
    #[default]
    Kufi,
}

/// The spelling written into restored quotations.
///
/// Both scripts are Tanzil's modern (imla'i) spelling and share one matching index, so
/// detection is identical; only the rendered vowel and tajweed marks differ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Script {
    /// Tanzil "Simple Plain" with pause marks: no special ikhfa or idgham marks.
    #[default]
    SimplePlain,
    /// Tanzil "Simple" with pause marks: shows ikhfa and idgham, as on Quran.com.
    Simple,
}

impl Script {
    /// Every supported script.
    pub const ALL: [Self; 2] = [Self::SimplePlain, Self::Simple];

    /// The stable name used by the command line and JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::SimplePlain => "simple-plain",
            Self::Simple => "simple",
        }
    }
}

impl fmt::Display for Script {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// The script name was not recognized.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown script {0:?}; expected simple-plain or simple")]
pub struct UnknownScript(pub String);

impl FromStr for Script {
    type Err = UnknownScript;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|script| script.name() == value)
            .ok_or_else(|| UnknownScript(value.to_owned()))
    }
}

/// The reading, verse count and script that matches and plans refer to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    /// The reading whose text is matched.
    pub riwaya: Riwaya,
    /// The verse-counting system used by references.
    pub counting: Counting,
    /// The spelling written into restored quotations.
    pub script: Script,
}

impl Edition {
    /// Hafs with the Kufan count, rendered in `script`.
    pub fn hafs(script: Script) -> Self {
        Self {
            riwaya: Riwaya::Hafs,
            counting: Counting::Kufi,
            script,
        }
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            riwaya: Riwaya::Hafs,
            counting: Counting::Kufi,
            script,
        } = self;
        write!(formatter, "hafs/kufi/{script}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_through_strings_and_json() {
        for script in Script::ALL {
            assert_eq!(script.name().parse::<Script>(), Ok(script));
        }
        assert!("uthmani".parse::<Script>().is_err());
        let edition = Edition::hafs(Script::Simple);
        let json = serde_json::to_string(&edition).unwrap();
        assert_eq!(
            json,
            r#"{"riwaya":"hafs","counting":"kufi","script":"simple"}"#
        );
        assert_eq!(serde_json::from_str::<Edition>(&json).unwrap(), edition);
        assert_eq!(edition.to_string(), "hafs/kufi/simple");
        assert_eq!(Edition::default(), Edition::hafs(Script::SimplePlain));
    }
}
