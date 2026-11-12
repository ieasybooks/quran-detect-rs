# quran-detect

Detect Quran quotations in Arabic text and restore their canonical wording, as a Rust library and command-line tool. Works on prose, OCR output and speech transcripts, including damaged quotations with typos and missing, extra, merged or split words.

The `quran-detect` library and command match text against an embedded, unmodified copy of the [Tanzil](https://tanzil.net) Quran text (Hafs, Kufan verse count). Each accepted quotation is rewritten with the canonical, vocalized wording; text outside the quotations is preserved byte for byte, including markup, punctuation and line endings. Every change can be recorded in a plan and undone exactly.

## Install

Install with [mise](https://mise.jdx.dev/):

```sh
mise use -g github:ieasybooks/quran-detect-rs@latest
```

Download the archive for your platform from the [latest release](https://github.com/ieasybooks/quran-detect-rs/releases/latest), extract it, and put `quran-detect` on your `PATH`. Archives support Linux x86_64/arm64, macOS arm64/x86_64 and Windows x86_64.

Or install with Cargo (Rust 1.99 or newer):

```sh
cargo install quran-detect-cli --locked
```

`quran-detect` needs no network access, model files or runtime libraries; the Tanzil text is compiled in.

## Usage

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
```

`--preset strict|balanced|tolerant` sets how damaged a quotation may be; balanced is the default. Strict accepts only exact normalized wording; balanced and tolerant repair more but can occasionally alter ordinary prose. Individual limits, the allowed errors, the exact-context guard and the rendered script (`simple-plain` or `simple`) can be set separately. Existing output files require `--force`. JSON offsets are UTF-8 bytes. See [matching and configuration](docs/matching.md), [JSON formats](docs/formats.md), the [CLI reference](docs/cli.md) and the [canonical text](docs/corpus.md).

Matching recognizes canonical wording; it does not establish that a writer meant to quote. Only the Hafs reading is embedded, so balanced mode also "corrects" valid readings from other riwayat to Hafs.

## Library

Add the library to an application:

```sh
cargo add quran-detect
```

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

A `Detector` is cheap to clone and safe to share between threads; the corpus index is built once per process, in about 0.2 seconds. See the [library README](crates/quran-detect/README.md) and the [API documentation](https://docs.rs/quran-detect).

## Documentation

- [Matching and configuration](docs/matching.md): how a match is found, presets, options, exact context, alternatives and formatting.
- [JSON formats](docs/formats.md): the match and plan formats.
- [Canonical text](docs/corpus.md): why Tanzil, its scripts and editions.
- [Validation](docs/validation.md): parity with the Python reference on 17,322 cases, benchmark results and speed.
- [Comparison](docs/comparison.md): against two IslamicEval 2026 shared-task models on OCR pages, lecture transcripts and the shared task's development set.
- [CLI reference](docs/cli.md): every option, example and exit code. Also available through `quran-detect --help`.
- [Man page](docs/quran-detect.1): included in release archives.
- API documentation: [`quran-detect`](https://docs.rs/quran-detect).

Install shell completions, then follow any printed setup instructions (replace `zsh` with your shell):

```sh
quran-detect completions zsh --install
```

## Development

Install the configured tools with mise, then run the checks from the repository root:

```sh
mise install
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check
```

After changing CLI declarations, regenerate the CLI reference and man page with `sh scripts/cli-docs.sh`; CI checks that they are current. [`scripts/generate-tables.py`](scripts/generate-tables.py) regenerates the frozen Unicode and HTML tables from CPython 3.13, and [`scripts/parity.py`](scripts/parity.py) compares this implementation with the Python reference; see [validation](docs/validation.md). See [AGENTS.md](AGENTS.md) for project conventions and the [release workflow](.github/RELEASING.md) for releases.

## License

MIT. The embedded Quran text is from the Tanzil Project, <https://tanzil.net>, under CC BY 3.0 and Tanzil's verbatim-copy terms; see [third-party notices](crates/quran-detect/THIRD-PARTY-NOTICES.md).
