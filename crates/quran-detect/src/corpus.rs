//! The embedded Tanzil text and its compact positional index.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use crate::edition::Script;
use crate::text::{Token, normalize, tokenize};
use crate::unicode::{is_space, nfc};

/// Verbatim Tanzil Simple Plain 1.1, downloaded with pause marks only.
pub(crate) const SIMPLE_PLAIN: &str = include_str!("../data/tanzil/quran-simple-plain.txt");
/// Verbatim Tanzil Simple 1.1, downloaded with pause marks only.
pub(crate) const SIMPLE: &str = include_str!("../data/tanzil/quran-simple.txt");
/// Verbatim Tanzil metadata 1.0, read for surah names and verse counts.
pub(crate) const METADATA: &str = include_str!("../data/tanzil/quran-data.xml");
/// Common words that cannot seed a match on their own.
pub(crate) const STOPWORDS: &str = include_str!("../data/stopwords.txt");

pub(crate) const SIMPLE_PLAIN_SHA256: &str =
    "e9f4aa7ba1bea8c5245dc75354e28fd448a423d5d8826f50295416634f506b80";
pub(crate) const SIMPLE_SHA256: &str =
    "01b4ee33d96ebc06964b2afab537833999f603feafff25a4378743115656c897";
#[cfg(test)]
pub(crate) const METADATA_SHA256: &str =
    "8867c1d88191472adec9db694b3cd9f135b1a2ef580574d32cf888dcb22c5c7a";
#[cfg(test)]
pub(crate) const STOPWORDS_SHA256: &str =
    "bf6d17d6bba54a3779840c37d757e7d4789ca00063a6eda04d09e9aec82de8da";

const SURAHS: usize = 114;
const KUFI_VERSES: usize = 6236;
const OPENING: [&str; 4] = ["بسم", "الله", "الرحمن", "الرحيم"];

/// The SHA-256 of the embedded text rendered for `script`.
pub(crate) fn text_sha256(script: Script) -> &'static str {
    match script {
        Script::SimplePlain => SIMPLE_PLAIN_SHA256,
        Script::Simple => SIMPLE_SHA256,
    }
}

/// One numbered verse and its interval of corpus word positions.
#[derive(Clone, Debug)]
pub(crate) struct Verse {
    pub(crate) surah: u16,
    pub(crate) number: u16,
    pub(crate) offset: usize,
    pub(crate) len: usize,
}

impl Verse {
    pub(crate) fn end(&self) -> usize {
        self.offset + self.len
    }
}

/// A parsed corpus line after removing Tanzil's unnumbered opening basmala.
struct Record {
    surah: u16,
    number: u16,
    text: Vec<char>,
    tokens: Vec<Token>,
}

/// The canonical verses, word positions and lookup tables shared by both scripts.
pub(crate) struct Corpus {
    pub(crate) verses: Vec<Verse>,
    pub(crate) names: Vec<String>,
    pub(crate) words: Vec<String>,
    pub(crate) owner: Vec<usize>,
    pub(crate) surah: Vec<u16>,
    pub(crate) local: Vec<usize>,
    positions: HashMap<String, Vec<usize>>,
    bigrams: HashMap<String, HashMap<String, Vec<usize>>>,
    joined: HashMap<String, Vec<usize>>,
    stopwords: HashSet<String>,
    canonical: [OnceLock<Vec<String>>; 2],
}

/// The shared corpus, built on first use.
pub(crate) fn corpus() -> &'static Corpus {
    static CORPUS: OnceLock<Corpus> = OnceLock::new();
    CORPUS.get_or_init(|| Corpus::load().expect("the embedded Tanzil corpus is valid"))
}

impl Corpus {
    fn load() -> Result<Self, String> {
        let surahs = surahs(METADATA)?;
        let records = records(SIMPLE_PLAIN, &surahs)?;
        let stopwords: Vec<&str> = lines(STOPWORDS).collect();
        let corpus = Self::new(
            &records,
            surahs.into_iter().map(|(name, _)| name).collect(),
            &stopwords,
        )?;
        corpus.canonical[0]
            .set(canonical_words(&records))
            .expect("unset");
        Ok(corpus)
    }

    fn new(records: &[Record], names: Vec<String>, stopwords: &[&str]) -> Result<Self, String> {
        let mut corpus = Self {
            verses: Vec::with_capacity(records.len()),
            names,
            words: Vec::new(),
            owner: Vec::new(),
            surah: Vec::new(),
            local: Vec::new(),
            positions: HashMap::new(),
            bigrams: HashMap::new(),
            joined: HashMap::new(),
            stopwords: HashSet::new(),
            canonical: [OnceLock::new(), OnceLock::new()],
        };
        let mut previous: Option<(u16, u16)> = None;
        for record in records {
            let key = (record.surah, record.number);
            if previous.is_some_and(|previous| key <= previous) {
                return Err("canonical verse identifiers must be unique and ordered".into());
            }
            previous = Some(key);
            if record.tokens.is_empty() {
                return Err(format!("empty canonical verse {}:{}", key.0, key.1));
            }
            let owner = corpus.verses.len();
            corpus.verses.push(Verse {
                surah: record.surah,
                number: record.number,
                offset: corpus.words.len(),
                len: record.tokens.len(),
            });
            for (local, token) in record.tokens.iter().enumerate() {
                let position = corpus.words.len();
                corpus
                    .positions
                    .entry(token.norm.clone())
                    .or_default()
                    .push(position);
                corpus.words.push(token.norm.clone());
                corpus.owner.push(owner);
                corpus.surah.push(record.surah);
                corpus.local.push(local);
                if position > 0 && corpus.surah[position - 1] == record.surah {
                    // Ordered consecutive verses only.
                    let previous = &corpus.verses[corpus.owner[position - 1]];
                    if previous.number == record.number || previous.number + 1 == record.number {
                        let left = &corpus.words[position - 1];
                        corpus
                            .bigrams
                            .entry(left.clone())
                            .or_default()
                            .entry(token.norm.clone())
                            .or_default()
                            .push(position - 1);
                        corpus
                            .joined
                            .entry(format!("{left}{}", token.norm))
                            .or_default()
                            .push(position - 1);
                    }
                }
            }
        }
        corpus.stopwords = stopwords
            .iter()
            .map(|word| normalize(&word.chars().collect::<Vec<_>>()))
            .filter(|word| !word.is_empty())
            .collect();
        Ok(corpus)
    }

    /// A small corpus of numbered verses for behavioral tests; every surah is named "س".
    #[cfg(test)]
    pub(crate) fn from_verses(verses: &[(u16, u16, &str)]) -> Self {
        let records: Vec<Record> = verses
            .iter()
            .map(|&(surah, number, text)| {
                let text: Vec<char> = text.chars().collect();
                let tokens = tokenize(&text);
                Record {
                    surah,
                    number,
                    text,
                    tokens,
                }
            })
            .collect();
        let corpus =
            Self::new(&records, vec!["س".to_owned(); SURAHS], &[]).expect("valid test corpus");
        corpus.canonical[0]
            .set(canonical_words(&records))
            .expect("unset");
        corpus
    }

    /// Canonical NFC words for `script`, aligned with `words`.
    pub(crate) fn canonical(&self, script: Script) -> &[String] {
        match script {
            Script::SimplePlain => self.canonical[0].get().expect("built with the index"),
            Script::Simple => self.canonical[1].get_or_init(|| {
                let surahs = surahs(METADATA).expect("validated metadata");
                let records = records(SIMPLE, &surahs).expect("the embedded Simple text is valid");
                let mut position = 0;
                for record in &records {
                    for token in &record.tokens {
                        assert!(
                            self.words.get(position) == Some(&token.norm),
                            "the Simple text must normalize to the Simple Plain index"
                        );
                        position += 1;
                    }
                }
                assert_eq!(position, self.words.len(), "Simple word count differs");
                canonical_words(&records)
            }),
        }
    }

    pub(crate) fn positions(&self, word: &str) -> &[usize] {
        self.positions.get(word).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn bigrams(&self, left: &str, right: &str) -> &[usize] {
        self.bigrams
            .get(left)
            .and_then(|rights| rights.get(right))
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn joined(&self, word: &str) -> &[usize] {
        self.joined.get(word).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn is_stopword(&self, word: &str) -> bool {
        self.stopwords.contains(word)
    }

    /// Adjacent positions in one surah and in the same or the next verse.
    pub(crate) fn connected(&self, left: isize, right: isize) -> bool {
        let (Ok(left), Ok(right)) = (usize::try_from(left), usize::try_from(right)) else {
            return false;
        };
        if left >= self.words.len() || right >= self.words.len() {
            return false;
        }
        if left.abs_diff(right) != 1 || self.surah[left] != self.surah[right] {
            return false;
        }
        let a = self.verses[self.owner[left]].number;
        let b = self.verses[self.owner[right]].number;
        a == b || a.abs_diff(b) == 1
    }

    /// The `(surah, verse)` path covered by word positions `start..end`.
    pub(crate) fn references(&self, start: usize, end: usize) -> Vec<(u16, u16)> {
        assert!(
            start < end && end <= self.words.len(),
            "invalid canonical interval"
        );
        let mut references = Vec::new();
        let mut position = start;
        while position < end {
            let verse = &self.verses[self.owner[position]];
            references.push((verse.surah, verse.number));
            position = end.min(verse.end());
        }
        references
    }
}

fn canonical_words(records: &[Record]) -> Vec<String> {
    records
        .iter()
        .flat_map(|record| {
            record
                .tokens
                .iter()
                .map(|token| nfc(&record.text[token.start..token.end]))
        })
        .collect()
}

/// Lines as CPython's `read_text(encoding="utf-8-sig").splitlines()` yields them for
/// files whose only line boundary is LF; a test checks the embedded files for that.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.split('\n')
}

fn records(text: &str, surahs: &[(String, usize)]) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    for (index, line) in lines(text).enumerate() {
        let line_no = index + 1;
        if line.chars().all(is_space) || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(3, '|');
        let (Some(surah), Some(number), Some(text)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(format!("malformed corpus line {line_no}"));
        };
        let (Ok(surah), Ok(number)) = (surah.parse::<u16>(), number.parse::<u16>()) else {
            return Err(format!("invalid verse identifier at line {line_no}"));
        };
        if !(1..=114).contains(&surah) || number < 1 {
            return Err(format!("invalid verse identifier at line {line_no}"));
        }
        let mut text: Vec<char> = text.chars().collect();
        let tokens = tokenize(&text);
        // Tanzil prefixes the unnumbered opening basmala to these lines.
        if surah != 1
            && number == 1
            && tokens.len() > 4
            && tokens[..4].iter().map(|t| t.norm.as_str()).eq(OPENING)
        {
            text.drain(..tokens[4].start);
        }
        let tokens = tokenize(&text);
        records.push(Record {
            surah,
            number,
            text,
            tokens,
        });
    }
    if records.len() != KUFI_VERSES {
        return Err(format!(
            "expected {KUFI_VERSES} canonical verses; found {}",
            records.len()
        ));
    }
    let mut expected = surahs.iter().enumerate().flat_map(|(index, (_, verses))| {
        let surah = u16::try_from(index + 1).expect("114 surahs");
        (1..=*verses).map(move |number| (surah, number))
    });
    for record in &records {
        let number = usize::from(record.number);
        if expected.next() != Some((record.surah, number)) {
            return Err(format!(
                "verse {}:{} does not follow the metadata order",
                record.surah, record.number
            ));
        }
    }
    Ok(records)
}

/// Surah names and verse counts from Tanzil's `<sura>` metadata elements.
fn surahs(xml: &str) -> Result<Vec<(String, usize)>, String> {
    let mut reader = Reader::from_str(xml);
    let mut surahs = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(element) | Event::Start(element))
                if element.name().as_ref() == b"sura" =>
            {
                let mut index = None;
                let mut verses = None;
                let mut name = None;
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|error| error.to_string())?;
                    let value = attribute
                        .decoded_and_normalized_value(XmlVersion::Explicit1_0, reader.decoder())
                        .map_err(|error| error.to_string())?
                        .into_owned();
                    match attribute.key.as_ref() {
                        b"index" => index = value.parse::<usize>().ok(),
                        b"ayas" => verses = value.parse::<usize>().ok(),
                        b"name" => name = Some(value),
                        _ => {}
                    }
                }
                let (Some(index), Some(verses), Some(name)) = (index, verses, name) else {
                    return Err("incomplete surah metadata".into());
                };
                if index != surahs.len() + 1 || verses == 0 || name.is_empty() {
                    return Err(format!("invalid metadata for surah {index}"));
                }
                surahs.push((name, verses));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    if surahs.len() != SURAHS
        || surahs.iter().map(|(_, verses)| verses).sum::<usize>() != KUFI_VERSES
    {
        return Err("metadata must describe 114 surahs and 6236 verses".into());
    }
    Ok(surahs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::sha256;

    #[test]
    fn embedded_files_are_the_pinned_verbatim_downloads() {
        assert_eq!(sha256(SIMPLE_PLAIN), SIMPLE_PLAIN_SHA256);
        assert_eq!(sha256(SIMPLE), SIMPLE_SHA256);
        assert_eq!(sha256(METADATA), METADATA_SHA256);
        assert_eq!(sha256(STOPWORDS), STOPWORDS_SHA256);
        // CPython splitlines() also breaks at these; the line reader assumes LF only.
        for text in [SIMPLE_PLAIN, SIMPLE, STOPWORDS] {
            assert!(!text.contains([
                '\r', '\u{0B}', '\u{0C}', '\u{1C}', '\u{1D}', '\u{1E}', '\u{85}', '\u{2028}',
                '\u{2029}'
            ]));
        }
    }

    #[test]
    fn index_has_every_verse_and_both_scripts_align() {
        let corpus = corpus();
        assert_eq!(corpus.verses.len(), 6236);
        assert_eq!(corpus.names.len(), 114);
        assert_eq!(corpus.names[1], "البقرة");
        assert_eq!(corpus.words.len(), 77800);
        let plain = corpus.canonical(Script::SimplePlain);
        let simple = corpus.canonical(Script::Simple);
        assert_eq!(plain.len(), corpus.words.len());
        assert_eq!(simple.len(), corpus.words.len());
        assert_ne!(plain, simple);
        // Al-Baqarah 2:1 starts after the unnumbered basmala.
        let baqarah = corpus
            .verses
            .iter()
            .find(|v| (v.surah, v.number) == (2, 1))
            .unwrap();
        assert_eq!(baqarah.len, 1);
        assert_eq!(corpus.words[baqarah.offset], "الم");
        assert_eq!(
            corpus.references(baqarah.offset, baqarah.offset + 3),
            [(2, 1), (2, 2)]
        );
        let first = isize::try_from(baqarah.offset).unwrap();
        assert!(!corpus.connected(first - 1, first));
        assert!(!corpus.connected(-1, 0));
        assert!(corpus.connected(0, 1));
    }
}
