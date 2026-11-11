# Matching and configuration

`quran-detect` identifies Quran wording in Arabic text and replaces each accepted fragment with the canonical text. It recognizes canonical wording, including unmarked phrases; it does not establish that a writer meant to quote. Text outside actual edits is preserved, including punctuation, markup, emoji and line endings.

## How a match is found

1. The input is tokenized into normalized Arabic words with their original offsets. Harakat, tatweel and shaping controls are ignored; hamza forms of alef, ta marbuta, alef maqsura and Farsi yeh and keheh are folded. Compatibility ligatures such as `ﷺ` and `ﻻ`, and HTML entities such as `&#1576;`, keep the interval of their original spelling. Markup tags, quotation marks, brackets, sentence punctuation, digits, symbols, non-Arabic letters and two consecutive line breaks separate matching blocks; a quotation never crosses them.
2. Exact word pairs, rare exact words, and words with one lost or extra space seed candidate positions in the corpus.
3. Each seed is extended in both directions with a bounded alignment that may use the allowed error kinds, within the edit budget.
4. Every candidate's edits are rendered and checked: re-tokenizing the edited span must give exactly the canonical words.
5. Automatic selection first chooses each span's best target (fewer edits, then alignment score, then corpus order), then the set of nonoverlapping spans with the highest total utility.

An attached prefix such as `و`, `ف`, `ب`, `ك`, `ل` or `وب` can stay outside the match: in `فيختم بقل هو الله أحد`, the match is `قل هو الله أحد`.

## Presets

| Preset | Maximum error fraction | Maximum edits | Minimum canonical words | Errors allowed |
|---|---:|---:|---:|---|
| `strict` | 0 | 0 | 3 | none: exact normalized matches |
| `balanced` (default) | 0.25 | 8 | 3 | typo, missing, extra, join, split |
| `tolerant` | 0.35 | 8 | 3 | balanced plus substitution |

Every preset requires two exact context words around word-changing errors (see below). Strict still restores canonical spelling and vocalization: it only requires the normalized words to match exactly. Balanced and tolerant can alter ordinary prose that happens to resemble a verse. Error budgets are edit limits, not confidence probabilities.

## Options

Each option overrides one value of the chosen preset. In the library, start from `Config::preset` and use its `with_*` methods.

| CLI | Library | Meaning |
|---|---|---|
| `--error-fraction F` | `with_error_fraction` | Maximum errors as a fraction of `max(input words, canonical words)`, from 0 to 1. |
| `--max-edits N` | `with_max_edits` | Maximum errors in one match. Both limits must pass. |
| `--min-words N` | `with_min_words` | Minimum canonical words in a match, at least 2. At least two input words must also support it. |
| `--allowed-errors LIST` | `with_allowed_errors` | Comma-separated error kinds, below. An empty value allows none. |
| `--min-exact-context N` or `B,A` | `with_min_exact_context` | Exact words required around word-changing errors. |
| `--script NAME` | `Detector::with_script` | `simple-plain` (default) or `simple`; see [corpus](corpus.md). |

Error kinds, each costing one edit:

- `typo`: one normalized character inserted, deleted or replaced; one of the two words has at least three letters.
- `missing`: insert an interior canonical word.
- `extra`: remove an interior input word.
- `join`: separate two canonical words merged into one input word.
- `split`: join a canonical word split across two input words.
- `substitution`: replace an unrelated interior word directly.

Allowing both `missing` and `extra` also permits replacing a word through a deletion and an insertion, at a cost of two. Balanced therefore disables direct one-edit substitution, not every whole-word change.

## Exact context

Each group of consecutive errors that contains `missing`, `extra` or `substitution` needs the configured number of consecutive exact words immediately before and after it. Those anchors and the canonical words in between must belong to one verse, so a long match cannot use its own words to justify completing a neighboring verse. Typo, join and split groups are not affected.

`N` sets both sides; `B,A` sets them separately, for example `1,2`. Zero on one side removes only that side's requirement; the verse check still applies, using the required side. Only `0` disables the check entirely. Higher values decline more word-changing repairs.

## Alternatives and explicit choices

`identify --alternatives` (library: `Detector::alternatives`) returns every executable candidate found by the bounded search, including overlaps, ordered by span. Each has `recommended`; the recommended subset equals `identify`. `score` is the interval utility used by selection, not a probability; sorting by it does not reproduce selection.

Pass any ordered, nonoverlapping subset to `--matches FILE` (library: `plan_matches` or `replace_matches`). Choices are applied exactly as given and never reranked; `[]` leaves the text unchanged. Matches only apply to the exact input they came from, in the same script.

When the same words occur in several places, `reference_alternatives` lists each occurrence that renders the match identically. Set a match's `reference_choice` to one of their `id`s to cite that occurrence instead; the choice must keep the same canonical text and verse path. `equally_ranked_targets` counts tied occurrences; ties use corpus order.

## Formatting

Formatting is off by default and never changes the matched span.

- `--append-reference` appends a citation such as `[البقرة - ٢٥٥]` for the selected occurrence. A partial quotation cites its verse; a quotation spanning verses cites a range such as `[الإخلاص - ١-٢]`. An identical adjacent citation is kept without duplication. A different adjacent citation is kept, no citation is added, and the plan reports a `formatting_conflicts` entry.
- `--brackets braces` converts a pair directly enclosing the quotation (`«»`, `“”`, `()`, `[]`, `﴿﴾` or `{}`, ignoring whitespace) to `{}` and places the citation after it. `braces-trimmed` also removes whitespace just inside the braces. No brackets are added when there are none; extra text, punctuation or an attached prefix between the quotation and the pair prevents conversion. A pair shared by several quotations is left unchanged.

## Limits

- Matching folds some spelling distinctions, so strict means zero normalized errors, not byte-identical text.
- Matching canonical words does not establish quotation intent: conventional phrases such as `بسم الله الرحمن الرحيم` are detected too.
- Only the Hafs reading is embedded. A valid reading from another riwaya can look like an error: balanced restores `ملك يوم الدين` to Hafs `مَالِكِ يَوْمِ الدِّينِ`. Strict leaves it unchanged.
- Repeating replacement on approximate presets is not guaranteed to leave the output unchanged.
- Longer approximate matches can move the boundaries of a correct shorter fragment. Exact undo proves reversibility, not correctness.
