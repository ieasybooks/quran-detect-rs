#!/usr/bin/env python3
"""Paired wall-clock timing of quran-detect and the Python reference on benchmark texts.

Both engines run in one long-lived process pinned to one core, timing identify, plan
and apply per document after loading their corpus once. Run on an otherwise idle machine:

    ../quran-matcher/.venv/bin/python -I scripts/timing.py \\
        --reference ../quran-matcher --archive ../quran-detector-experiments --output timing.json
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATASETS = {
    "islamiceval": "comparison/data/inference.jsonl",
    "ocr": "matcher_experiment/dataset/panel_inference.jsonl",
    "faqieh-windows": "faqieh_10h/dataset/inference.jsonl",
}
PRESETS = ("strict", "balanced", "tolerant")


def texts(archive: Path) -> dict[str, list[tuple[str, str]]]:
    result = {}
    for name, relative in DATASETS.items():
        with (archive / relative).open(encoding="utf-8") as stream:
            rows = [json.loads(line) for line in stream]
        result[name] = [(r["id"], r.get("text", r.get("generated_answer"))) for r in rows]
    return result


def summary(seconds: list[float]) -> dict:
    return {
        "documents": len(seconds),
        "total_seconds": round(sum(seconds), 3),
        "median_ms": round(1000 * statistics.median(seconds), 3),
        "p95_ms": round(1000 * sorted(seconds)[int(0.95 * (len(seconds) - 1))], 3),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if hasattr(os, "sched_setaffinity"):
        os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})
    sys.path.insert(0, str(args.reference.resolve() / "src"))
    from quran_matcher import MatchConfig, QuranMatcher, apply_plan

    data = texts(args.archive.resolve())
    subprocess.run(
        ["cargo", "build", "--release", "--locked", "-p", "quran-detect", "--example", "timing"],
        cwd=ROOT,
        check=True,
    )
    binary = ROOT / "target/release/examples/timing"
    report = {"cpu": os.cpu_count(), "results": {}}
    for preset in PRESETS:
        python = QuranMatcher(MatchConfig.preset(preset))
        python.restore("قل هو الله أحد")
        for name, documents in data.items():
            python_seconds = []
            for _, text in documents:
                started = time.perf_counter()
                matches = python.identify(text)
                apply_plan(text, python.plan(text, matches))
                python_seconds.append(time.perf_counter() - started)
            request = "".join(
                json.dumps({"id": i, "text": t, "preset": preset}, ensure_ascii=False) + "\n"
                for i, t in documents
            )
            completed = subprocess.run(
                [str(binary)], input=request.encode(), capture_output=True, check=True
            )
            rust_seconds = [json.loads(line)["seconds"] for line in completed.stdout.splitlines()]
            assert len(rust_seconds) == len(documents)
            python_summary, rust_summary = summary(python_seconds), summary(rust_seconds)
            report["results"][f"{name}/{preset}"] = {
                "python": python_summary,
                "rust": rust_summary,
                "speedup": round(python_summary["total_seconds"] / rust_summary["total_seconds"], 1),
            }
            print(name, preset, report["results"][f"{name}/{preset}"], flush=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
