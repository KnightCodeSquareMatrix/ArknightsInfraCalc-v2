#!/usr/bin/env python3
"""Call the local narrator against a PlayerTrainingAdviceReport v2 JSON file."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

# Allow direct execution from the repository without installing the package.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from arknights_narrator.adapter import Narrator
from arknights_narrator.mapping import CanonicalMapping, MappingError
from arknights_narrator.projection import project_report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True, help="PlayerTrainingAdviceReport v2 JSON")
    parser.add_argument("--mapping", type=Path, default=Path("data/canonical_mapping.json"))
    parser.add_argument("--env-file", type=Path, default=Path(".env"))
    parser.add_argument("--output", type=Path, help="Optional JSON output path")
    args = parser.parse_args()

    _load_env_file(args.env_file)
    try:
        report = json.loads(args.report.read_text(encoding="utf-8"))
        mapping = CanonicalMapping.load(args.mapping)
        result = Narrator().generate(project_report(report, mapping))
    except (OSError, ValueError, MappingError, json.JSONDecodeError) as exc:
        print(f"narrator smoke test failed before LLM call: {exc}", file=sys.stderr)
        return 2

    payload = {
        "status": result.status,
        "fallback_reason": result.fallback_reason,
        "narrative": result.report,
        "structured_report": report,
    }
    encoded = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded, encoding="utf-8")
    else:
        print(encoded, end="")
    return 0 if result.status == "ready" else 1


def _load_env_file(path: Path) -> None:
    if not path.exists():
        return
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            raise ValueError(f"invalid .env line: {line}")
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip())


if __name__ == "__main__":
    raise SystemExit(main())
