#!/usr/bin/env python3
"""Build the external narrator name mapping from current player knowledge."""

from __future__ import annotations

import argparse
import json
import os
import tempfile
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--knowledge", type=Path, default=Path("data/training_advice_knowledge.json"))
    parser.add_argument("--output", type=Path, default=Path("data/canonical_mapping.json"))
    parser.add_argument("--version", required=True, help="monotonically increasing dotted mapping version")
    parser.add_argument("--instances", type=Path, default=Path("data/operator_instances.json"))
    parser.add_argument("--skill-table", type=Path, default=Path("data/skill_table.json"))
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
        for selector in entry.get("dynamic_skill_members", []):
            needle = selector.get("skill_name_contains")
            facility = selector.get("facility")
            if not isinstance(needle, str) or not isinstance(facility, str):
                continue
            operators.update(_dynamic_operators(args.instances, args.skill_table, needle, facility))

    payload = {
        "mapping_version": args.version,
        "locale": "zh-CN",
        "operators": {
            name: {"name": name, "aliases": []} for name in sorted(operators)
        },
        "combinations": {
            identifier: {"name": name, "aliases": []}
            for identifier, name in sorted(combinations.items())
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{args.output.name}.", dir=args.output.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            stream.write(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, args.output)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    return 0


def _dynamic_operators(instances_path: Path, skill_table_path: Path, needle: str, facility: str) -> set[str]:
    instances = json.loads(instances_path.read_text(encoding="utf-8"))
    skills = json.loads(skill_table_path.read_text(encoding="utf-8"))
    skill_names = {
        entry.get("id"): entry.get("skill_name", "")
        for entry in skills.get("skills", [])
        if isinstance(entry, dict)
    }
    operators: set[str] = set()
    for instance in instances.get("instances", {}).values():
        binding = instance.get("facilities", {}).get(facility)
        if binding is None and facility == "manufacture":
            binding = instance.get("facilities", {}).get("manufacture")
        if not isinstance(binding, dict):
            continue
        if any(needle in skill_names.get(buff_id, "") for buff_id in binding.get("buff_ids", [])):
            name = instance.get("name")
            if isinstance(name, str):
                operators.add(name)
    return operators


if __name__ == "__main__":
    raise SystemExit(main())
