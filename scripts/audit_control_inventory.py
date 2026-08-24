#!/usr/bin/env python3
"""Extract a fail-closed, row-per-atom control-center inventory."""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path

FIELDS = [
    "csv_row",
    "skill",
    "operator",
    "tier",
    "buff_id",
    "atom_id",
    "effect_key",
    "status",
    "reason",
]
TIER_NAMES = {"精0": "tier_0", "精1": "tier_1", "精2": "tier_2"}


def buff_stem(buff_id: str) -> str:
    return buff_id.rsplit("[", 1)[0] if "[" in buff_id else buff_id


def resolve_buff_ids(tier: str, binding: dict, tier0_binding: dict | None) -> list[str]:
    ids = list(binding.get("buff_ids", []))
    if tier == "tier_0" or not binding.get("stepwise") or not tier0_binding:
        return ids
    resolved = list(tier0_binding.get("buff_ids", []))
    for buff_id in ids:
        resolved = [item for item in resolved if buff_stem(item) != buff_stem(buff_id)]
        resolved.append(buff_id)
    return resolved


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def extract_inventory(registry_path: Path, instances_path: Path, skills_path: Path) -> tuple[list[dict], list[str]]:
    registry = list(csv.DictReader(registry_path.open(encoding="utf-8-sig", newline="")))
    instances = load_json(instances_path)["instances"]
    skills = {skill["id"]: skill for skill in load_json(skills_path)["skills"]}
    rows: list[dict] = []
    errors: list[str] = []
    seen: set[tuple[str, str, str, str]] = set()

    for csv_row, source in enumerate(registry, start=2):
        if source.get("工作设施") != "控制中枢":
            continue
        operator = source["干员"]
        tier_label = source["需求精英"]
        tier = TIER_NAMES.get(tier_label)
        if tier is None:
            errors.append(f"row {csv_row}: unsupported tier {tier_label!r}")
            continue
        instance = instances.get(f"{operator}@{tier}")
        if instance is None:
            errors.append(f"row {csv_row}: missing instance {operator}@{tier}")
            continue
        control = instance.get("facilities", {}).get("control")
        if control is None:
            errors.append(f"row {csv_row}: missing control binding for {operator}@{tier}")
            continue
        tier0 = instances.get(f"{operator}@tier_0", {}).get("facilities", {}).get("control")
        buff_ids = resolve_buff_ids(tier, control, tier0)
        if not buff_ids:
            errors.append(f"row {csv_row}: empty control buff binding for {operator}@{tier}")
        for buff_id in buff_ids:
            skill = skills.get(buff_id)
            if skill is None:
                errors.append(f"row {csv_row}: missing skill_table entry {buff_id}")
                rows.append(_row(csv_row, source, tier, buff_id, None, "missing skill_table entry"))
                continue
            if skill.get("facility") != "control":
                errors.append(f"row {csv_row}: {buff_id} has facility {skill.get('facility')!r}")
            atoms = skill.get("atoms", [])
            if not atoms:
                errors.append(f"row {csv_row}: {buff_id} has empty atoms")
                rows.append(_row(csv_row, source, tier, buff_id, None, "empty atoms require semantic classification"))
                continue
            for atom in atoms:
                atom_id = atom.get("atom_id")
                if not atom_id:
                    errors.append(f"row {csv_row}: {buff_id} has atom without atom_id")
                key = (operator, tier, buff_id, atom_id or "")
                if key in seen:
                    errors.append(f"row {csv_row}: duplicate mapping {key}")
                seen.add(key)
                rows.append(_row(csv_row, source, tier, buff_id, atom_id, "EffectKey classification required"))
    return rows, errors


def _row(csv_row: int, source: dict, tier: str, buff_id: str, atom_id: str | None, reason: str) -> dict:
    return {
        "csv_row": str(csv_row),
        "skill": source.get("技能名", ""),
        "operator": source.get("干员", ""),
        "tier": tier,
        "buff_id": buff_id,
        "atom_id": atom_id or "",
        "effect_key": "",
        "status": "unsupported",
        "reason": reason,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--instances", type=Path, required=True)
    parser.add_argument("--skills", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    rows, errors = extract_inventory(args.registry, args.instances, args.skills)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=FIELDS)
        writer.writeheader()
        writer.writerows(rows)
    for error in errors:
        print(f"ERROR: {error}", file=sys.stderr)
    print(f"wrote {len(rows)} inventory rows to {args.output}")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
