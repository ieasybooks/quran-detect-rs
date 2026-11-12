# Comparison with shared-task models

`quran-detect` 0.1.0 compared with two span detectors from the [IslamicEval 2026 shared task](https://sites.google.com/view/islamiceval2026/subtask-1), on OCR book pages, lecture transcripts and the shared task's development set.

## Datasets

| Dataset | Documents | Labels |
|---|---|---|
| [Quran Quotations in OCR Book Pages](https://huggingface.co/datasets/ieasybooks-org/quran-detect-ocr) | 500 scanned pages from 25 books, with their OCR text | 1,593 quotations, each with the canonical texts it may be restored to |
| [Quran Quotations in Lecture Transcripts](https://huggingface.co/datasets/ieasybooks-org/quran-detect-asr) | 39 recorded lectures (26.3 hours) with their speech-recognition transcripts | 1,302 quotations with their canonical texts, plus unresolved quotations, Quran wording used in passing and protected paraphrases |
| [IslamicEval 2026 Subtask 1 development set](https://sites.google.com/view/islamiceval2026/subtask-1) | 484 answers written by language models to questions about Islam | The organizers' Quran and Hadith spans, with corrections for misquoted verses |

From the development set, detection is scored on the 298 answers that contain no claim the organizers mark as uncorrectable; 99 of them have no Quran span. For restoration, each official correction was aligned with its misquoted fragment before any system output was read, and the fragments within one or two word edits were kept after review: 26 fragments in 24 answers, of which 8 differ by spelling or one extra word and the rest by a substituted word or phrase. 221 correctly quoted fragments serve as controls that must stay unchanged.

## Systems

- **quran-detect 0.1.0**: the crates.io release with its three presets, run on whole documents. [Matching and configuration](matching.md) describes how it finds and restores quotations.
- **SanadAI**: the team's released [AraModernBERT span tagger](https://huggingface.co/AbirKorched9/SanadAI-AraModernBERT-IslamicNER) (revision `d22a63b`) with the team's inference code and postprocessing ([paper](https://aclanthology.org/2026.arabicnlp-sharedtasks.131/)).
- **Bayan_CU reproduction**: the team published code but no trained model ([paper](https://aclanthology.org/2026.arabicnlp-sharedtasks.133/)). We trained [AraBERTv2](https://huggingface.co/aubmindlab/bert-base-arabertv2) following their code, with its unchanged prediction and postprocessing functions, on the official training set: 4,123 answers, three epochs, learning rate 3e-5, seed 42. It is not the team's submitted model.

Both models tag Quran and Hadith spans but do not rewrite them; only their Quran spans are used. To compare restoration, their spans go through one canonical restorer: a span that cuts through a word is widened to the whole word, then rewritten with the closest canonical Quran wording. The restorer corrects letters but adds no vowel marks to wording whose letters already match, so the letter-based columns measure its restoration.

## How it was run

- OCR pages and shared-task answers reach every system whole. The models read each lecture transcript in windows of whole cues, up to 2,200 characters, with two cues of context on each side; a span belongs to the window it starts in, and of two overlapping spans the longer is kept. quran-detect reads each transcript whole.
- OCR and transcript scores come from each dataset's evaluator, `evaluation/evaluate.py`. Every system's predictions are in the datasets' `baselines/` folders, so the tables can be reproduced and new systems added.
- Shared-task detection is character-level precision, recall and F1 of Quran spans against the organizers' Quran spans. A fragment is repaired when its words in the output equal the official correction, ignoring optional vowel marks and common letter variants.

## OCR book pages

| System | Known recall | Restored (letters) | Restored (vocalized) | Edits outside labels (letter-changing) |
|---|---:|---:|---:|---:|
| quran-detect 0.1.0, strict | 85.43% | 1,137 / 1,593 | 1,103 / 1,593 | 1,176 (49) |
| quran-detect 0.1.0, balanced | 93.84% | 1,390 / 1,593 | 1,355 / 1,593 | 1,303 (128) |
| quran-detect 0.1.0, tolerant | 95.76% | 1,430 / 1,593 | 1,394 / 1,593 | 2,707 (690) |
| SanadAI + restorer | 81.05% | 1,308 / 1,593 | 813 / 1,593 | 55 (37) |
| Bayan_CU reproduction + restorer | 60.17% | 1,250 / 1,593 | 535 / 1,593 | 26 (13) |

Known recall is the share of labelled quotation characters detected; the labels are not exhaustive, so precision is not measured. A quotation is restored when its words in the output equal a target exactly, without the optional vowel marks (letters) or with all of them (vocalized). Edits outside labels change unlabelled text; most only add vowel marks, and the letter-changing ones are in parentheses.

## Lecture transcripts

| System | Precision | Recall | F1 | Restored (normalized) | Word edits outside labels / protected |
|---|---:|---:|---:|---:|---:|
| quran-detect 0.1.0, strict | 95.97% | 85.98% | 90.70% | 1,012 / 1,302 | 0 / 0 |
| quran-detect 0.1.0, balanced | 94.17% | 90.10% | 92.09% | 1,124 / 1,302 | 86 / 27 |
| quran-detect 0.1.0, tolerant | 78.83% | 91.88% | 84.85% | 1,148 / 1,302 | 787 / 47 |
| SanadAI + restorer | 89.02% | 30.30% | 45.21% | 1,069 / 1,302 | 15 / 1 |
| Bayan_CU reproduction + restorer | 77.68% | 37.06% | 50.18% | 1,083 / 1,302 | 25 / 3 |

Detection is character-level against the dataset's detection spans, outside ignored spans. A quotation is restored when its words in the output equal a target, ignoring vowel marks and common letter variants. Word edits count changed words in unlabelled speech and in protected paraphrases.

## Shared task

| System | Precision | Recall | F1 | Spelling and extra-word repairs | All small-corruption repairs | Correct quotations changed |
|---|---:|---:|---:|---:|---:|---:|
| quran-detect 0.1.0, strict | 86.77% | 65.74% | 74.80% | 0 / 8 | 0 / 26 | 0 / 221 |
| quran-detect 0.1.0, balanced | 86.67% | 67.88% | 76.13% | 6 / 8 | 6 / 26 | 0 / 221 |
| quran-detect 0.1.0, tolerant | 79.35% | 69.98% | 74.37% | 6 / 8 | 7 / 26 | 0 / 221 |
| SanadAI + restorer | 99.02% | 99.28% | 99.15% | 6 / 8 | 7 / 26 | 0 / 221 |
| Bayan_CU reproduction + restorer | 99.25% | 99.39% | 99.32% | 6 / 8 | 7 / 26 | 0 / 221 |

Detection covers the 298 scored answers. Repairs count the 8 spelling and extra-word fragments and all 26 fragments described above; the last column counts changed controls.
