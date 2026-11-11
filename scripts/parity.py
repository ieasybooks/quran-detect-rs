#!/usr/bin/env python3
"""Compare quran-detect with the Python reference matcher, output by output.

The Rust port keeps the reference algorithm, so every match, alternative, edit, plan and
output must be identical once the reference uses the same Tanzil corpus and its
code-point offsets are converted to UTF-8 byte offsets. Run from the repository root
with the reference checkout's interpreter (Python 3.13 and its Levenshtein package):

    ../quran-matcher/.venv/bin/python -I scripts/parity.py \\
        --reference ../quran-matcher --archive ../quran-detector-experiments \\
        --output /tmp/quran-detect-parity

Exit status 0 means no difference. The report lists every difference otherwise.
"""

from __future__ import annotations

import argparse
import copy
import dataclasses
import hashlib
import io
import json
import multiprocessing
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "crates/quran-detect/data"
TEXT_FILES = {"simple-plain": "quran-simple-plain.txt", "simple": "quran-simple.txt"}
SHA256 = {
    "simple-plain": "e9f4aa7ba1bea8c5245dc75354e28fd448a423d5d8826f50295416634f506b80",
    "simple": "01b4ee33d96ebc06964b2afab537833999f603feafff25a4378743115656c897",
}
FORMATTINGS = [
    ({}, {"append_reference": False, "brackets": "keep"}),
    ({"append_reference": True}, {"append_reference": True, "brackets": "keep"}),
    (
        {"append_reference": True, "normalize_brackets": True},
        {"append_reference": True, "brackets": "braces"},
    ),
    (
        {"append_reference": True, "normalize_brackets": True, "trim_bracket_whitespace": True},
        {"append_reference": True, "brackets": "braces-trimmed"},
    ),
    (
        {"normalize_brackets": True, "trim_bracket_whitespace": True},
        {"append_reference": False, "brackets": "braces-trimmed"},
    ),
]
PRESETS = ("strict", "balanced", "tolerant")
VARIANTS = {
    "balanced-context-0": ("balanced", {"min_exact_context": 0}),
    "balanced-context-1-2": ("balanced", {"min_exact_context": (1, 2)}),
    "typo-only": ("balanced", {"allowed_errors": frozenset({"typo"})}),
    "structural-only": (
        "balanced",
        {"allowed_errors": frozenset({"missing", "extra", "join", "split"})},
    ),
    "join-only": ("tolerant", {"allowed_errors": frozenset({"join"})}),
    "tolerant-wide": (
        "tolerant",
        {"error_fraction": 1.0, "max_edits": 3, "min_words": 2, "min_exact_context": (0, 1)},
    ),
}
EDGE_CASES = [
    "",
    "   \n\t ",
    "No Arabic text here.",
    "فيختم بقل هو الله أحد",
    "قال: «الله لا إله إلا الحي القيوم»، ثم أكمل.",
    "الله لا اله الا هو الحى القيوم لا تأخذه سنه ولا نوم",
    "<p>قال تعالى: <b>قل هو الله</b> أحد</p>",
    "قل هو &#1575;&#1604;&#1604;&#1607; أحد &amp; الله الصمد",
    "قل هو الله &nbsp;أحد &foo; الله الصمد &notit; لم يلد",
    "قل هو الله أحد <b الله الصمد",
    "﷽ الحمد لله رب العالمين",
    "قال رسول الله ﷺ إنما الأعمال بالنيات، وقال تعالى قل هو الله أحد",
    "قل هو الله أحد ﻻ إله إلا الله وحده",
    "ﺑﺴﻢ ﺍﻟﻠﻪ ﺍﻟﺮﺣﻤﻦ ﺍﻟﺮﺣﻴﻢ ﺍﻟﺤﻤﺪ ﻟﻠﻪ ﺭﺏ ﺍﻟﻌﺎﻟﻤﻴﻦ",
    "قـــل هـو الله أحـد‍ الله الصمد",
    "قل هو الله أحد\r\nالله الصمد\r\n\r\nلم يلد ولم يولد",
    "الحمد لله العالمين الرحمن الرحيم مالك يوم الدين",
    "الحمد لله رب جميع العالمين الرحمن الرحيم مالك يوم الدين",
    "الحمدلله رب العالمين الرحمن الرحيم مالك يوم الدين",
    "الحمد لله رب العا لمين الرحمن الرحيم مالك يوم الدين",
    "الحمد لله رب الناس الرحمن الرحيم مالك يوم الدين",
    "{قل هو الله أحد} [الإخلاص - ١] ثم «قل هو الله أحد» [البقرة - ٢]",
    "( قل هو الله أحد ) و﴿ الله الصمد لم يلد ﴾ و“ولم يولد ولم يكن له كفوا أحد”",
    "[قل هو الله أحد] [الإخلاص - ١-٢]",
    "قل هو الله أحد الله الصمد لم يلد ولم يولد ولم يكن له كفوا أحد",
    "فبأي آلاء ربكما تكذبان",
    "قال تعالى فبأي آلاء ربكما تكذبان ثم قال فبأي آلاء ربكما تكذبان",
    "بسم الله الرحمن الرحيم",
    "قل يا أيها الكافرون لا أعبد ما تعبدون",
    "قل هو الله أحد الله الصمد لم یلد ولم یولد",
    "Verse 2:255 الله لا إله إلا هو الحي القيوم 255",
    "وقل رب زدني علما",
    "&#1608;قل رب زدني علما",
    "وَقُلْ رَبِّ زِدْنِي عِلْمًا",
    "فبما رحمة من الله لنت لهم ولو كنت فظا غليظ القلب",
    "ولقد آتينا موسى الكتاب وقفينا من بعده بالرسل",
    "قُلْ هُوَ اللَّهُ أَحَدٌ",
    "أولئك على هدى من ربهم وأولئك هم المفلحون",
    "قل هو الله\nأحد الله الصمد\n\nلم يلد ولم يولد",
    "الله لا إله إلا هو، الحي القيوم؛ لا تأخذه سنة ولا نوم",
    "قل هو اللهࢗأحد الله الصمد",
    "قل هو الله أ́حد الله الصمد",
    "ذلك الكتاب لا ريب ۛ فيه ۛ هدى للمتقين",
    "الرَّحْمَٰنِ الرَّحِيمِ مَالِكِ يَوْمِ الدِّينِ",
    "إنا أعطيناك الكوثر فصل لربك وانحر إن شانئك هو الأبتر",
    "قل أعوذ برب الناس ملك الناس إله الناس من شر الوسواس الخناس",
]


def sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def rows(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as stream:
        return [json.loads(line) for line in stream]


def load_texts(archive: Path, reference: Path) -> list[tuple[str, str, bool]]:
    """(id, text, long document) for every benchmark input and edge case."""
    sources = [
        ("islamiceval", archive / "comparison/data/inference.jsonl", False),
        ("ocr", archive / "matcher_experiment/dataset/panel_inference.jsonl", False),
        ("faqieh-window", archive / "faqieh_10h/dataset/inference.jsonl", False),
        ("faqieh-document", archive / "faqieh_10h/dataset/documents.jsonl", True),
        ("faqieh-fresh", reference / "research/datasets/faqieh-fresh-inference.jsonl", True),
        ("walkthrough-window", reference / "runs/walkthrough/confirmation/dataset/input.jsonl", False),
        ("walkthrough-document", reference / "runs/walkthrough/confirmation/dataset/documents.jsonl", True),
    ]
    texts = []
    for name, path, long in sources:
        for row in rows(path):
            text = row.get("text", row.get("generated_answer"))
            assert isinstance(text, str), (name, row.get("id"))
            texts.append((f"{name}/{row['id']}", text, long))
    texts += [(f"edge/{i}", text, False) for i, text in enumerate(EDGE_CASES)]
    return texts


def corpus_directories(reference: Path, work: Path) -> dict[str, Path]:
    """Reference corpus folders holding the same Tanzil text the crate embeds."""
    package = reference / "src/quran_matcher/data/audited-v1"
    names = [s.get("name") for s in ET.parse(package / "quran-index.xml").getroot().findall("sura")]
    tanzil = [s.get("name") for s in ET.parse(DATA / "tanzil/quran-data.xml").getroot().iter("sura")]
    assert names == tanzil, "surah names must match the embedded Tanzil metadata"
    directories = {}
    for script, file in TEXT_FILES.items():
        directory = work / script
        directory.mkdir(parents=True)
        shutil.copyfile(DATA / "tanzil" / file, directory / "quran-simple.txt")
        shutil.copyfile(package / "quran-index.xml", directory / "quran-index.xml")
        shutil.copyfile(DATA / "stopwords.txt", directory / "nonTerminals.txt")
        assert sha256((directory / "quran-simple.txt").read_text(encoding="utf-8")) == SHA256[script]
        directories[script] = directory
    return directories


# ---------------------------------------------------------------- reference worker

STATE: dict = {}


def init_worker(reference: str, directories: dict[str, str]) -> None:
    sys.path.insert(0, str(Path(reference) / "src"))
    import quran_matcher  # noqa: F401

    STATE["directories"] = directories
    STATE["corpora"] = {}
    STATE["matchers"] = {}


def matcher(config_name: str, script: str):
    from quran_matcher import MatchConfig, QuranMatcher
    from quran_matcher._corpus import Corpus

    key = (config_name, script)
    if key not in STATE["matchers"]:
        if script not in STATE["corpora"]:
            STATE["corpora"][script] = Corpus.load(STATE["directories"][script])
        preset, overrides = (config_name, {}) if config_name in PRESETS else VARIANTS[config_name]
        config = dataclasses.replace(MatchConfig.preset(preset), **overrides)
        STATE["matchers"][key] = QuranMatcher(config, corpus=STATE["corpora"][script])
    return STATE["matchers"][key]


def offsets(text: str) -> list[int]:
    result = [0]
    for char in text:
        result.append(result[-1] + len(char.encode("utf-8")))
    return result


def rust_config(described: dict) -> dict:
    context = described["min_exact_context"]
    return {
        "error_fraction": float(described["error_fraction"]),
        "max_edits": described["max_edits"],
        "min_words": described["min_words"],
        "allowed_errors": sorted(described["allowed_errors"]),
        "min_exact_context": [context, context] if isinstance(context, int) else list(context),
    }


def rust_formatting(python: dict) -> dict:
    if not python.get("normalize_brackets"):
        brackets = "keep"
    elif python.get("trim_bracket_whitespace"):
        brackets = "braces-trimmed"
    else:
        brackets = "braces"
    return {"append_reference": bool(python.get("append_reference")), "brackets": brackets}


def convert_edit(edit: dict, table: list[int]) -> dict:
    result = {
        "start": table[edit["start"]],
        "end": table[edit["end"]],
        "before": edit["before"],
        "after": edit["after"],
        "kinds": list(edit["kinds"]),
    }
    if "proposal_id" in edit:
        result["proposal_id"] = edit["proposal_id"]
    return result


def byte_id(text: str, table: list[int], start: int, end: int, edits: list[dict]) -> str:
    from quran_matcher.core import _span_output

    rendered = _span_output(text, start, end, edits)
    return sha256(f"{sha256(text)}:{table[start]}:{table[end]}:{rendered}")


def convert_match(match: dict, text: str, table: list[int], edition: dict) -> dict:
    result = {
        "start": table[match["start"]],
        "end": table[match["end"]],
        "text": match["text"],
        "canonical_text": match["canonical_text"],
        "references": match["references"],
        "reference_start": match["reference_start"],
        "reference_end": match["reference_end"],
        "alignment_errors": match["alignment_errors"],
        "error_fraction": match["error_fraction"],
        "equally_ranked_targets": match["equally_ranked_targets"],
        "edits": [convert_edit(e, table) for e in match["edits"]],
        "input_sha256": match["input_sha256"],
        "recommended": match["recommended"],
        "score": match["score"],
        "score_method": match["score_method"],
        "reference_alternatives": match["reference_alternatives"],
        "edition": edition,
        "id": byte_id(text, table, match["start"], match["end"], match["edits"]),
    }
    if "reference_choice" in match:
        result["reference_choice"] = match["reference_choice"]
    return result


def convert_plan(plan: dict, text: str, script: str, edition: dict) -> dict:
    from quran_matcher import apply_plan

    table = offsets(text)
    output = apply_plan(text, plan)
    proposals = []
    for proposal in plan["proposals"]:
        converted = {
            "id": proposal["id"],
            "candidate_id": byte_id(
                text, table, proposal["start"], proposal["end"], proposal["lexical_edits"]
            ),
            "start": table[proposal["start"]],
            "end": table[proposal["end"]],
            "text": proposal["text"],
            "before": proposal["before"],
            "canonical_text": proposal["canonical_text"],
            "references": proposal["references"],
            "reference_start": proposal["reference_start"],
            "reference_end": proposal["reference_end"],
            "alignment_errors": proposal["alignment_errors"],
            "error_fraction": proposal["error_fraction"],
            "equally_ranked_targets": proposal["equally_ranked_targets"],
            "input_sha256": proposal["input_sha256"],
            "recommended": proposal["recommended"],
            "score": proposal["score"],
            "score_method": proposal["score_method"],
            "reference_alternatives": proposal["reference_alternatives"],
            "edition": edition,
            "status": proposal["status"],
            "reason": proposal["reason"],
            "lexical_edits": [convert_edit(e, table) for e in proposal["lexical_edits"]],
            "formatting_edits": [convert_edit(e, table) for e in proposal["formatting_edits"]],
            "formatting_conflicts": [
                {**c, "start": table[c["start"]], "end": table[c["end"]]}
                for c in proposal["formatting_conflicts"]
            ],
            "edits": [convert_edit(e, table) for e in proposal["edits"]],
        }
        if "reference_choice" in proposal:
            converted["reference_choice"] = proposal["reference_choice"]
        proposals.append(converted)
    return {
        "version": "2.0",
        "detector_version": plan["detector_version"],
        "config": rust_config(plan["config"]),
        "formatting": rust_formatting(plan["formatting"]),
        "edition": edition,
        "offset_unit": "UTF-8 bytes, [start,end)",
        "input_sha256": plan["input_sha256"],
        "output_sha256": plan["output_sha256"],
        "input_bytes": len(text.encode("utf-8")),
        "output_bytes": len(output.encode("utf-8")),
        "corpus_sha256": SHA256[script],
        "proposals": proposals,
        "edits": [convert_edit(e, table) for e in plan["edits"]],
        "summary": plan["summary"],
    }


def compatible_alternatives(candidates: list[dict]) -> list[dict]:
    """Non-recommended candidates first, greedily nonoverlapping, in source order."""
    chosen = []
    for candidate in sorted(candidates, key=lambda c: (c["recommended"], c["start"], c["end"])):
        if all(candidate["end"] <= c["start"] or c["end"] <= candidate["start"] for c in chosen):
            chosen.append(candidate)
    return sorted(chosen, key=lambda c: (c["start"], c["end"]))


def reference_case(task: tuple[str, str, str, str]) -> tuple[str, dict, dict, float]:
    key, text, config_name, script = task
    from quran_matcher import apply_plan, revert_plan

    reference = matcher(config_name, script)
    edition = {"riwaya": "hafs", "counting": "kufi", "script": script}
    started = time.perf_counter()
    candidates = reference.identify(text, alternatives=True)
    elapsed = time.perf_counter() - started
    matches = [c for c in candidates if c["recommended"]]
    table = offsets(text)
    plans, outputs = [], []
    for python, _ in FORMATTINGS:
        plan = reference.plan(text, matches, **python)
        output = apply_plan(text, plan)
        assert revert_plan(output, plan) == text
        plans.append(convert_plan(plan, text, script, edition))
        outputs.append(output)
    choices, expected_choices = [], []

    def add_choice(selected: list[dict], python: dict) -> None:
        try:
            expected = convert_plan(reference.plan(text, selected, **python), text, script, edition)
        except ValueError:
            expected = {"error": True}
        choices.append(
            {
                "matches": [convert_match(m, text, table, edition) for m in selected],
                "formatting": rust_formatting(python),
            }
        )
        expected_choices.append(expected)

    full = {"append_reference": True, "normalize_brackets": True}
    if any(len(m["reference_alternatives"]) > 1 for m in matches):
        chosen = copy.deepcopy(matches)
        for m in chosen:
            if len(m["reference_alternatives"]) > 1:
                m["reference_choice"] = m["reference_alternatives"][-1]["id"]
        add_choice(chosen, full)
        unknown = copy.deepcopy(matches)
        unknown[0]["reference_choice"] = "0:0"
        add_choice(unknown, full)
    alternatives = compatible_alternatives(candidates)
    if alternatives and alternatives != matches:
        add_choice(alternatives, full)
    if matches:
        add_choice([], full)
        add_choice(list(reversed(matches)) if len(matches) > 1 else matches, {})
    request = {
        "id": key,
        "text": text,
        "config": rust_config(reference.config.describe()),
        "script": script,
        "formattings": [rust for _, rust in FORMATTINGS],
        "choices": choices,
    }
    expected = {
        "id": key,
        "identify": [convert_match(m, text, table, edition) for m in matches],
        "alternatives": [convert_match(c, text, table, edition) for c in candidates],
        "plans": plans,
        "outputs": outputs,
        "reverts": [True] * len(FORMATTINGS),
        "choices": expected_choices,
    }
    return key, request, expected, elapsed


# ---------------------------------------------------------------- comparison


def first_difference(expected, actual, path="$"):
    if isinstance(expected, dict) and isinstance(actual, dict):
        if expected.keys() != actual.keys():
            return f"{path}: keys {sorted(expected.keys() ^ actual.keys())}"
        for key in expected:
            found = first_difference(expected[key], actual[key], f"{path}.{key}")
            if found:
                return found
        return None
    if isinstance(expected, list) and isinstance(actual, list):
        if len(expected) != len(actual):
            return f"{path}: length {len(expected)} != {len(actual)}"
        for i, (a, b) in enumerate(zip(expected, actual)):
            found = first_difference(a, b, f"{path}[{i}]")
            if found:
                return found
        return None
    if type(expected) is not type(actual) and not (
        isinstance(expected, (int, float)) and isinstance(actual, (int, float))
    ):
        return f"{path}: {expected!r} != {actual!r}"
    return None if expected == actual else f"{path}: {expected!r} != {actual!r}"


def compare(expected: dict, actual: dict) -> str | None:
    actual = copy.deepcopy(actual)
    expected = copy.deepcopy(expected)
    # Error messages differ by language; only whether an error occurred must agree.
    for i, choice in enumerate(expected["choices"]):
        if choice.get("error") is True and "error" in actual["choices"][i]:
            actual["choices"][i] = {"error": True}
    return first_difference(expected, actual)


def run_reference(args, tasks, requests_path: Path, expected_path: Path) -> tuple[float, float]:
    """Run every case through the Python reference, writing results as they arrive."""
    print(f"reference: {len(tasks)} cases on {args.jobs} workers", flush=True)
    work = Path(tempfile.mkdtemp(prefix="quran-detect-parity-"))
    python_seconds = 0.0
    started = time.perf_counter()
    try:
        directories = {k: str(v) for k, v in corpus_directories(args.reference.resolve(), work).items()}
        with (
            multiprocessing.get_context("spawn").Pool(
                args.jobs, init_worker, (str(args.reference.resolve()), directories)
            ) as pool,
            requests_path.open("w", encoding="utf-8") as requests,
            expected_path.open("w", encoding="utf-8") as expected,
        ):
            for done, (_, request, result, elapsed) in enumerate(
                pool.imap_unordered(reference_case, tasks, chunksize=1), 1
            ):
                requests.write(json.dumps(request, ensure_ascii=False) + "\n")
                expected.write(json.dumps(result, ensure_ascii=False) + "\n")
                python_seconds += elapsed
                if done % 500 == 0 or done == len(tasks):
                    print(f"reference: {done}/{len(tasks)} cases", flush=True)
    finally:
        shutil.rmtree(work)
    return python_seconds, time.perf_counter() - started


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--jobs", type=int, default=max(1, (multiprocessing.cpu_count() or 2) - 1))
    parser.add_argument("--quick", action="store_true", help="Edge cases and 20 texts per source")
    parser.add_argument(
        "--reuse",
        type=Path,
        help="Reuse requests.jsonl and expected.jsonl from an earlier output; skip the reference",
    )
    args = parser.parse_args()
    if args.output.exists():
        parser.error(f"{args.output} exists; choose a fresh directory")
    args.output.mkdir(parents=True)
    texts = load_texts(args.archive.resolve(), args.reference.resolve())
    if args.quick:
        seen: dict[str, int] = {}
        kept = []
        for key, text, long in texts:
            source = key.split("/")[0]
            seen[source] = seen.get(source, 0) + 1
            if source == "edge" or seen[source] <= 20:
                kept.append((key, text, long))
        texts = kept
    tasks = []
    for key, text, long in texts:
        for script in TEXT_FILES:
            for preset in PRESETS:
                tasks.append((f"{key}|{preset}|{script}", text, preset, script))
        if not long:
            for variant in VARIANTS:
                tasks.append((f"{key}|{variant}|simple-plain", text, variant, "simple-plain"))
    tasks.sort(key=lambda task: -len(task[1]))
    print(f"{len(texts)} texts, {len(tasks)} cases", flush=True)
    if args.reuse:
        requests_path = args.reuse / "requests.jsonl"
        expected_path = args.reuse / "expected.jsonl"
        python_seconds = reference_wall = 0.0
    else:
        requests_path = args.output / "requests.jsonl"
        expected_path = args.output / "expected.jsonl"
        python_seconds, reference_wall = run_reference(args, tasks, requests_path, expected_path)
    subprocess.run(
        ["cargo", "build", "--release", "--locked", "-p", "quran-detect", "--example", "parity"],
        cwd=ROOT,
        check=True,
    )
    binary = ROOT / "target/release/examples/parity"
    # Results arrive in request order, so all three streams are compared line by line in
    # constant memory; whole runs produce several gigabytes of JSON.
    counts = dict.fromkeys(
        ("cases", "matches", "alternatives", "edits", "choice_plans", "changed_outputs"), 0
    )
    keys, differences = set(), []
    started = time.perf_counter()
    with (
        requests_path.open("rb") as stdin,
        requests_path.open(encoding="utf-8") as requests,
        expected_path.open(encoding="utf-8") as expected_lines,
        subprocess.Popen([str(binary)], stdin=stdin, stdout=subprocess.PIPE) as process,
    ):
        for request_line, expected_line, actual_line in zip(
            requests, expected_lines, io.TextIOWrapper(process.stdout, encoding="utf-8"), strict=True
        ):
            request, expected, actual = map(json.loads, (request_line, expected_line, actual_line))
            assert request["id"] == expected["id"] == actual["id"], "streams out of order"
            keys.add(request["id"])
            counts["cases"] += 1
            counts["matches"] += len(expected["identify"])
            counts["alternatives"] += len(expected["alternatives"])
            counts["edits"] += sum(len(p["edits"]) for p in expected["plans"])
            counts["choice_plans"] += len(expected["choices"])
            counts["changed_outputs"] += sum(o != request["text"] for o in expected["outputs"])
            difference = compare(expected, actual)
            if difference:
                differences.append({"case": request["id"], "difference": difference})
    if process.returncode != 0:
        raise SystemExit(f"parity example exited with {process.returncode}")
    rust_wall = time.perf_counter() - started
    assert keys == {task[0] for task in tasks}, "the saved cases differ from this selection"
    report = {
        "status": "identical" if not differences else "different",
        "counts": {"texts": len(texts), **counts},
        "reference_identify_cpu_seconds": round(python_seconds, 1),
        "reference_wall_seconds": round(reference_wall, 1),
        "rust_wall_seconds_all_cases": round(rust_wall, 1),
        "differences": differences,
    }
    (args.output / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps({k: v for k, v in report.items() if k != "differences"}, indent=2))
    for difference in differences[:20]:
        print(difference)
    sys.exit(1 if differences else 0)


if __name__ == "__main__":
    main()
