# quran-detect

Detect Quran quotations in Arabic text and restore their canonical wording. The crate embeds an unmodified copy of the [Tanzil](https://tanzil.net) Quran text (Hafs, Kufan verse count) and needs no files, network or runtime libraries.

```rust
use quran_detect::{Config, Detector, Formatting, Preset};

let detector = Detector::new(Config::preset(Preset::Strict));
let text = "فيختم بقل هو الله أحد";
let matches = detector.identify(text);
assert_eq!(&text[matches[0].start..matches[0].end], "قل هو الله أحد");
assert_eq!(matches[0].references[0].surah(), 112);
assert_eq!(detector.replace(text, Formatting::default()), "فيختم بقُلْ هُوَ اللَّهُ أَحَدٌ");
```

Restore with citations, keep a plan and undo it exactly:

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

- `Detector::identify` returns recommended, nonoverlapping matches; `alternatives` adds overlapping candidates. Offsets are UTF-8 byte offsets.
- `replace` restores the text; `plan` records every edit with input and output hashes, and `Plan::apply` and `Plan::revert` replay or undo it exactly.
- `plan_matches` and `replace_matches` apply an explicit selection, such as a filtered `alternatives` list or a match with `reference_choice` set.
- `Config` starts from a `Preset` (strict, balanced or tolerant) and adjusts the error fraction, edit limit, minimum words, allowed `ErrorKind`s and `ExactContext`.
- `Formatting` optionally appends citations such as `[البقرة - ٢٥٥]` and converts enclosing brackets to braces.
- `Script::SimplePlain` (default) or `Script::Simple` selects the rendered spelling; detection is the same for both.

`Match` and `Plan` are `serde` types matching the command-line JSON. A `Detector` is `Clone + Send + Sync`; the corpus index is built once per process on first use. Matching is synchronous and CPU-bound; async servers should call it on a blocking worker.

Matching recognizes canonical wording; it does not establish that a writer meant to quote. See the repository's [matching and configuration](https://github.com/ieasybooks/quran-detect-rs/blob/main/docs/matching.md) and [JSON formats](https://github.com/ieasybooks/quran-detect-rs/blob/main/docs/formats.md).

Code is MIT licensed. The embedded Quran text is from the Tanzil Project, <https://tanzil.net>, under CC BY 3.0 and Tanzil's verbatim-copy terms; see [third-party notices](THIRD-PARTY-NOTICES.md).
