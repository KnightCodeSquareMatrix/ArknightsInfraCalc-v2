#!/usr/bin/env python3
"""Batch-check fixed 12/12 control states with the Rust effect assertion."""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GROUPS = {
    "representative": ROOT / "data/fixtures/box-bundles/collected/collected_operboxes",
    "high-owned": ROOT / "data/fixtures/box-bundles/high-owned",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--group", choices=["representative", "high-owned", "all"], default="all")
    parser.add_argument("--assert-invariants", action="store_true")
    args = parser.parse_args()
    groups = GROUPS.items() if args.group == "all" else [(args.group, GROUPS[args.group])]
    binary = ROOT / "target/debug/infra-cli"
    subprocess.run(["cargo", "build", "-p", "infra-cli"], cwd=ROOT, check=True)

    failures = {"invariant": [], "solver": [], "input": [], "timeout": []}
    total = 0
    for group, directory in groups:
        boxes = sorted(path for path in directory.glob("*.json") if not path.stem.endswith("_profile"))
        for box in boxes:
            total += 1
            with tempfile.TemporaryDirectory() as temporary:
                command = [
                    str(binary),
                    "plan",
                    "--operbox",
                    str(box),
                    "--rotation",
                    "2",
                    "--no-fiammetta",
                    "--profile-out",
                    str(Path(temporary) / "profile.json"),
                ]
                if args.assert_invariants:
                    command.append("--assert-invariants")
                try:
                    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=180)
                except subprocess.TimeoutExpired:
                    failures["timeout"].append((group, box.name, "timeout after 180s"))
                    continue
            if result.returncode:
                reason = result.stderr.strip().splitlines()[-1] if result.stderr.strip() else "failed"
                if "assert-invariants" in reason:
                    category = "invariant"
                elif "operbox parse" in reason or "invalid type" in reason:
                    category = "input"
                else:
                    category = "solver"
                failures[category].append((group, box.name, reason))

    failed = sum(len(items) for items in failures.values())
    print(f"checked={total} failed={failed}")
    for category, items in failures.items():
        print(f"{category}_failures={len(items)}")
        for group, name, reason in items:
            print(f"FAIL {category} {group}/{name}: {reason}")
    return int(bool(failed))


if __name__ == "__main__":
    sys.exit(main())
