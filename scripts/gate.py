#!/usr/bin/env python3
"""Compare benchmark summaries of quran-detect with the Python reference's.

Both directories hold `<dataset>/<preset>/summary.json`, as written by the reference
repository's `python -m benchmarks run` (with `--engine rust --binary ...` for this
project). Rates are compared in percentage points; for damage, lower is better.

The release gate: on each dataset and preset, no headline rate may be more than 0.5
points worse, and the sum of signed differences across all of them must not be negative.

    python3 -I scripts/gate.py --python RUNS/python --rust RUNS/rust --output gate.md
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

TOLERANCE = 0.5
PRESETS = ("strict", "balanced", "tolerant")


def rates(summary: dict) -> dict[str, tuple[float, bool]]:
    """Headline rates in percent, each with whether higher is better."""
    result: dict[str, tuple[float, bool]] = {}
    detection = summary["detection"]
    for key in ("precision", "recall", "f1", "known_recall"):
        if key in detection:
            result[f"detection {key}"] = (100 * detection[key], True)
    for view, values in summary.get("replacement", {}).items():
        targets = values["targets"]
        result[f"{view} correct"] = (100 * values["final_correct"] / targets, True)
        result[f"{view} repaired"] = (100 * values["newly_repaired"] / targets, True)
        result[f"{view} damaged"] = (100 * values["newly_damaged"] / targets, False)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--python", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    datasets = sorted(p.name for p in args.python.iterdir() if p.is_dir())
    lines = [
        "| Dataset | Preset | Metric | Python 0.2.0 | quran-detect | Difference |",
        "|---|---|---|---:|---:|---:|",
    ]
    failures, total, compared = [], 0.0, 0
    for dataset in datasets:
        for preset in PRESETS:
            python = json.loads((args.python / dataset / preset / "summary.json").read_text())
            rust = json.loads((args.rust / dataset / preset / "summary.json").read_text())
            assert python["documents"] == rust["documents"], (dataset, preset)
            assert python["roundtrips_verified"] == rust["roundtrips_verified"] == rust["documents"]
            left, right = rates(python), rates(rust)
            assert left.keys() == right.keys(), (dataset, preset)
            for metric, (before, higher) in left.items():
                after = right[metric][0]
                signed = (after - before) if higher else (before - after)
                total += signed
                compared += 1
                if signed < -TOLERANCE:
                    failures.append(f"{dataset} {preset} {metric}: {signed:+.3f}")
                lines.append(
                    f"| {dataset} | {preset} | {metric} | {before:.3f} | {after:.3f} | {after - before:+.3f} |"
                )
    verdict = "PASS" if not failures and total >= 0 else "FAIL"
    lines += [
        "",
        f"{compared} rates compared. Sum of signed differences (positive is better): {total:+.3f} points.",
        f"Gate: {verdict}.",
    ]
    lines += [f"- worse than {TOLERANCE} points: {failure}" for failure in failures]
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines[-(3 + len(failures)) :]))
    sys.exit(0 if verdict == "PASS" else 1)


if __name__ == "__main__":
    main()
