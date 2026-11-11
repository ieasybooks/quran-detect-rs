# Tanzil Quran text

These files are unmodified downloads from the [Tanzil Project](https://tanzil.net). They are embedded in the `quran-detect` library and checked against the hashes below by its tests. Do not edit them: Tanzil's terms permit verbatim copies only, and the detector's results depend on these exact bytes.

| File | Tanzil text | Download options | SHA-256 |
| --- | --- | --- | --- |
| `quran-simple-plain.txt` | Simple Plain, version 1.1 | Text with aya numbers, pause marks only | `e9f4aa7ba1bea8c5245dc75354e28fd448a423d5d8826f50295416634f506b80` |
| `quran-simple.txt` | Simple, version 1.1 | Text with aya numbers, pause marks only | `01b4ee33d96ebc06964b2afab537833999f603feafff25a4378743115656c897` |
| `quran-data.xml` | Quran metadata, version 1.0 | — | `8867c1d88191472adec9db694b3cd9f135b1a2ef580574d32cf888dcb22c5c7a` |

Downloaded on 2026-11-11 from:

- `https://tanzil.net/pub/download/index.php?quranType=simple-plain&outType=txt-2&agree=true&marks=true`
- `https://tanzil.net/pub/download/index.php?quranType=simple&outType=txt-2&agree=true&marks=true`
- `https://tanzil.net/res/text/metadata/quran-data.xml`

Both texts are Tanzil's modern (imla'i) spelling of the Hafs reading, verified by Tanzil against the Medina Mushaf, with the Kufan count of 6,236 verses. Simple marks ikhfa and idgham (for example `مِّن رَّبِّهِمْ`); Simple Plain does not (`مِنْ رَبِّهِمْ`). Both normalize to the same words, so the library builds one matching index from Simple Plain and renders either script. Tanzil prefixes the unnumbered opening basmala to verse 1 of each surah except Al-Fatiha and At-Tawba; the library removes that prefix when it loads the text. Sajdah signs, rub-el-hizb signs and tatweel options were left off.

Tanzil's terms of use, also reproduced at the end of each text file:

- Permission is granted to copy and distribute verbatim copies of the text, but changing it is not allowed.
- The text can be used in any website or application, provided that its source (Tanzil Project) is clearly indicated, and a link is made to tanzil.net to enable users to keep track of changes.
- The copyright notice shall be included in all verbatim copies of the text, and reproduced appropriately in all files derived from or containing a substantial portion of it.

To update, download with the same options, review [Tanzil's update log](https://tanzil.net/updates/), replace the files, and update the hashes in `src/corpus.rs`, this file and `scripts/parity.py`. Then run the parity check and the benchmark gate described in [validation](../../../../docs/validation.md) before releasing.

`../stopwords.txt` is part of this project, not Tanzil: common Arabic words that cannot seed a match on their own.
