# quran-detect-rs

Detect Quran quotations in Arabic text and restore their canonical wording, as a Rust library and command-line tool. Works on prose, OCR output and speech transcripts, including damaged quotations with typos and missing, extra, merged or split words.

The `quran-detect` library and command match text against an embedded, unmodified copy of the [Tanzil](https://tanzil.net) Quran text (Hafs, Kufan verse count). Each accepted quotation is rewritten with the canonical, vocalized wording; text outside the quotations is preserved byte for byte, including markup, punctuation and line endings. Every change can be recorded in a plan and undone exactly.

## Build

Use the pinned Rust toolchain (1.99.1):

```sh
cargo build --release --locked
```

The executable is `target/release/quran-detect`. It needs no network access, model files or runtime libraries; the Tanzil text is compiled in.

## Use

```sh
quran-detect --text 'فيختم بقل هو الله أحد'
# فيختم بقُلْ هُوَ اللَّهُ أَحَدٌ
quran-detect book.txt -o book.restored.txt
quran-detect --text '«الله لا إله إلا الحي القيوم»' --append-reference --brackets braces
# {اللَّهُ لَا إِلَٰهَ إِلَّا هُوَ الْحَيُّ الْقَيُّومُ} [البقرة - ٢٥٥]
printf 'قل هو الله احد' | quran-detect -

quran-detect identify book.txt                      # quotations as JSON
quran-detect identify --alternatives book.txt       # with overlapping candidates
quran-detect book.txt --matches chosen.json         # apply an edited selection
quran-detect plan book.txt -o plan.json             # record every edit
quran-detect apply book.txt --plan plan.json -o restored.txt
quran-detect revert restored.txt --plan plan.json -o original.txt
quran-detect completions zsh --install
```

`--preset strict|balanced|tolerant` sets how damaged a quotation may be; balanced is the default. Strict accepts only exact normalized wording; balanced and tolerant repair more but can occasionally alter ordinary prose. Individual limits, the allowed errors, the exact-context guard and the rendered script (`simple-plain` or `simple`) can be set separately. Existing output files require `--force`. JSON offsets are UTF-8 bytes. See [matching and configuration](docs/matching.md), [JSON formats](docs/formats.md), the [CLI reference](docs/cli.md) and the [canonical text](docs/corpus.md).

Matching recognizes canonical wording; it does not establish that a writer meant to quote. Only the Hafs reading is embedded, so balanced mode also "corrects" valid readings from other riwayat to Hafs.

## Library

```rust
use quran_detect::{Brackets, Config, Detector, Formatting, Preset, Script};

let detector = Detector::new(Config::preset(Preset::Balanced)).with_script(Script::SimplePlain);
let text = "قال: «الله لا إله إلا الحي القيوم»";
for found in detector.identify(text) {
    println!("{:?} {} -> {}", found.references, &text[found.start..found.end], found.canonical_text);
}
let options = Formatting { append_reference: true, brackets: Brackets::Braces };
let plan = detector.plan(text, options);
let restored = plan.apply(text)?;
assert_eq!(plan.revert(&restored)?, text);
# Ok::<(), quran_detect::Error>(())
```

A `Detector` is cheap to clone and safe to share between threads; the corpus index is built once per process, in about 0.2 seconds. See the [library README](crates/quran-detect/README.md).

## Accuracy

The matching algorithm is a port of the Python reference implementation (`quran-matcher` 0.2). On 17,322 cases (every benchmark text and edge case, the three presets in both scripts, plus six variant configurations), its matches, alternatives, edits, plans and outputs are identical to the reference running on the same Tanzil text. Against the reference's own benchmark results on its older corpus, 112 of 138 rates are identical and the rest are at most 0.27 points lower, all because of two verses Tanzil corrected in version 1.1 (17:32 and 8:6). See [validation](docs/validation.md).

## Development

```sh
mise install
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check
sh scripts/cli-docs.sh   # after changing CLI declarations
```

[`scripts/generate-tables.py`](scripts/generate-tables.py) regenerates the frozen Unicode and HTML tables from CPython 3.13, and [`scripts/parity.py`](scripts/parity.py) compares this implementation with the Python reference; see [validation](docs/validation.md). Releases: [release workflow](.github/RELEASING.md).

Code is MIT licensed. The embedded Quran text is from the Tanzil Project, <https://tanzil.net>, under CC BY 3.0 and Tanzil's verbatim-copy terms; see [third-party notices](crates/quran-detect/THIRD-PARTY-NOTICES.md).
