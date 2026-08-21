#!/usr/bin/env python3
"""Build the external narrator name mapping from current player knowledge."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--knowledge", type=Path, default=Path("data/training_advice_knowledge.json"))
    parser.add_argument("--output", type=Path, default=Path("data/canonical_mapping.json"))
    args = parser.parse_args()

    knowledge = json.loads(args.knowledge.read_text(encoding="utf-8"))
    operators: set[str] = set()
    combinations: dict[str, str] = {}
    for entry in knowledge.get("newbie", []):
        operators.add(entry["operator"])
    for entry in knowledge.get("standalone", []):
        operators.add(entry["operator"])
    for entry in knowledge.get("combinations", []):
        combinations[entry["id"]] = entry["name"]
        for member in entry.get("members", []):
            operators.add(member["operator"])
        for group in entry.get("completion", {}).get("choice_groups", []):
            operators.update(group.get("members", []))
        for alternative in entry.get("completion", {}).get("alternatives", []):
            for required in alternative.get("required", []):
                operators.add(required)
            for group in alternative.get("choice_groups", []):
                operators.update(group.get("members", []))

    payload = {
        "mapping_version": "1.0.0",
        "locale": "zh-CN",
        "operators": {
            name: {"name": name, "aliases": []} for name in sorted(operators)
        },
        "combinations": {
            identifier: {"name": name, "aliases": []}
            for identifier, name in sorted(combinations.items())
        },
    }
    args.output.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
