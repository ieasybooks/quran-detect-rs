# Canonical text

The library embeds unmodified Tanzil downloads: the Hafs reading in Tanzil's modern (imla'i) spelling, verified by Tanzil against the Medina Mushaf, with the Kufan count of 6,236 verses. File details, hashes and terms are in [`crates/quran-detect/data/tanzil`](../crates/quran-detect/data/tanzil/README.md).

## Why Tanzil

The detector needs fully vocalized text in modern spelling, because OCR pages, transcripts and prose are written in modern spelling, with clear reuse terms. Tanzil provides that, with a public change log, under CC BY 3.0 with a verbatim-copy condition. Alternatives checked in November 2026:

- The King Fahd Complex publishes the Medina Mushaf in Uthmani script for its own fonts; its modern-spelling field is unvocalized search text, and no reuse terms were found.
- Quran.com's modern-spelling text (`text_imlaei`) is Tanzil Simple 1.0.2 with pause marks: every verse matches except the six corrections Tanzil published in version 1.1 (2:181, 5:31, 8:6, 13:37, 17:32, 39:56). QUL offers the same script type with no license information.
- Matching modern spelling against Uthmani script misses about 12% of words even for Hafs (`الصلاة`/`الصلوة`, `الكتاب`/`الكتب`).

## Scripts

`--script` (library: `Detector::with_script`) selects the spelling written into restored quotations:

| Script | Tanzil text | Example |
|---|---|---|
| `simple-plain` (default) | Simple Plain 1.1, pause marks | `هُدًى مِنْ رَبِّهِمْ ۖ` |
| `simple` | Simple 1.1, pause marks; marks ikhfa and idgham as on Quran.com | `هُدًى مِّن رَّبِّهِمْ ۖ` |

Both scripts normalize to the same 77,800 words, so detection, spans and references are identical; only the rendered vowel and tajweed marks differ, in 3,933 verses. A test checks this when the Simple text is first used. Restored words are NFC-normalized, which can reorder combining marks without changing how they display.

## Editions

Every match and plan records its `edition`: the reading (`riwaya`), the verse-counting system (`counting`) and the script. Version 0.1 supports `hafs` with the `kufi` count only. Other readings (Warsh, Qalun, al-Duri, al-Susi, al-Bazzi, Qunbul, Shu'ba) would need modern-spelling text that is not freely available; verse counts also differ (6,214 for Warsh and Qalun, 6,217 for al-Duri and al-Susi, 6,220 for al-Bazzi and Qunbul), which changes citations. The JSON formats and library types already carry the edition so that adding one does not change them.
