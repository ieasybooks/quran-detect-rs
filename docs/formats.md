# JSON formats

All offsets are UTF-8 byte offsets into the input, as half-open `[start, end)` ranges, so `input[start..end]` is the matched text in Rust. Other languages must convert: Python and Ruby index strings by code point, JavaScript by UTF-16 unit. Canonical positions (`reference_start`, `reference_end`) count corpus words, not bytes.

## Match

`quran-detect identify` prints an array of matches; the library type is `Match`.

| Field | Meaning |
|---|---|
| `start`, `end` | Byte span of the matched input text. |
| `text` | `input[start..end]`. |
| `canonical_text` | The canonical words that replace it. |
| `references` | The `[surah, verse]` path of the selected occurrence. |
| `reference_start`, `reference_end` | Canonical word positions of the selected occurrence. |
| `alignment_errors` | Non-exact operations used. |
| `error_fraction` | `alignment_errors / max(input words, canonical words)`. |
| `equally_ranked_targets` | Occurrences tied with the selected one. |
| `edits` | Edits inside the span: `start`, `end`, `before`, `after`, `kinds`. |
| `input_sha256` | SHA-256 of the whole input; the match only applies to that text. |
| `recommended` | Whether automatic selection chose it. |
| `score`, `score_method` | Interval utility used by selection; not a probability. |
| `reference_alternatives` | Occurrences that render it identically: `id`, `reference_start`, `reference_end`, `references`. |
| `edition` | `{"riwaya": "hafs", "counting": "kufi", "script": "simple-plain"}`. |
| `id` | SHA-256 of `"{input_sha256}:{start}:{end}:{restored span}"`; equal outputs share an identity. |
| `reference_choice` | Optional; set it to an alternative's `id` before planning. |

`error_fraction` is a floating-point number written in its shortest exact form. Applications reading matches with Rust's `serde_json` should enable its `float_roundtrip` feature, as the CLI does, so the value is parsed back exactly.

Edit `kinds` are, in name order: `exact` (spelling or vocalization restored), `extra`, `join`, `missing`, `split`, `substitution`, `typo`, and in plans `bracket` and `reference`. Matches are read back strictly: unknown fields are rejected, so a misspelled `reference_choice` is an error rather than ignored.

## Plan

`quran-detect plan` prints a plan; the library type is `Plan`. Version `2.0` uses byte offsets.

| Field | Meaning |
|---|---|
| `version` | `"2.0"`. Plans written by the Python reference (`"1.0"`, code-point offsets) are not accepted. |
| `detector_version` | `"lexical-matcher-2"`, the matching algorithm. |
| `config`, `formatting`, `edition` | The options that produced it. `min_exact_context` is `[before, after]`. |
| `offset_unit` | `"UTF-8 bytes, [start,end)"`. |
| `input_sha256`, `output_sha256` | Hashes checked by apply and revert. |
| `input_bytes`, `output_bytes` | Lengths in bytes. |
| `corpus_sha256` | SHA-256 of the embedded Tanzil text that was rendered. |
| `proposals` | One per match: its match fields, `id` (one-based), `candidate_id` (the match `id`), `before`, `status` (`replace` or `unchanged`), `reason`, `lexical_edits`, `formatting_edits`, `formatting_conflicts` and `edits`. |
| `edits` | Every edit in source order, each with its `proposal_id`. |
| `summary` | Proposal counts: `replace`, `unchanged`. |

`apply` checks the input hash, applies the edits and checks the output hash. `revert` checks the output hash, applies the inverse edits and checks the input hash. Neither rebases offsets onto changed text.
