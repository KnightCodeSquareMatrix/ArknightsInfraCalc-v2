#!/usr/bin/env python3
"""Audit building-skill bindings against ArknightsGameResource unlock phases.

The runtime intentionally has two effective states: tier_0 and tier_up.
For 3/4-star operators tier_up is the E1 state; for 5/6-star operators it is
the E2 state. This audit compares the effective buff set in both states while
retaining the upstream phase information for diagnostics.
"""

from __future__ import annotations

import argparse
import json
import urllib.request
from collections import defaultdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM = "https://raw.githubusercontent.com/yuanyan3060/ArknightsGameResource/main/gamedata/excel"
FACILITIES = {
    "CONTROL": "control",
    "MANUFACT": "manufacture",
    "TRADE": "trade",
    "POWER": "power",
    "DORMITORY": "dorm",
    "HIRE": "office",
    "MEETING": "meeting",
    "TRAINING": "training",
    "WORKSHOP": "workshop",
}


def fetch_json(name: str) -> dict[str, Any]:
    with urllib.request.urlopen(f"{UPSTREAM}/{name}", timeout=60) as response:
        return json.load(response)


def load_sources(source_dir: Path | None) -> tuple[dict[str, Any], dict[str, Any]]:
    if source_dir is not None:
        return (
            json.loads((source_dir / "character_table.json").read_text(encoding="utf-8")),
            json.loads((source_dir / "building_data.json").read_text(encoding="utf-8")),
        )
    return fetch_json("character_table.json"), fetch_json("building_data.json")


def facility_for(buff: dict[str, Any], buff_id: str) -> str:
    room_type = buff.get("roomType")
    if room_type in FACILITIES:
        return FACILITIES[room_type]
    return {
        "control": "control",
        "trade": "trade",
        "manu": "manufacture",
        "power": "power",
    }.get(buff_id.split("_", 1)[0], "unknown")


def buff_stem(buff_id: str) -> str:
    return buff_id.rsplit("[", 1)[0]


def runtime_target_phase(source_rarity: int) -> int:
    """Return the phase represented by runtime tier_up.

    Upstream rarity is zero-based: 3 means a 4-star operator, 4/5 means
    5/6-star. The project runtime uses E1 as tier_up for 3/4-star operators
    and E2 as tier_up for 5/6-star operators.
    """

    return 1 if source_rarity <= 3 else 2


def latest_phase_rows(rows: list[dict[str, Any]], max_phase: int) -> set[str]:
    by_stem: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        if row["phase"] <= max_phase:
            by_stem[buff_stem(row["buff_id"])].append(row)
    out: set[str] = set()
    for stem_rows in by_stem.values():
        phase = max(row["phase"] for row in stem_rows)
        out.update(row["buff_id"] for row in stem_rows if row["phase"] == phase)
    return out


def merge_stepwise(tier0: list[str], tier_up: list[str]) -> set[str]:
    out = list(tier0)
    for buff_id in tier_up:
        stem = buff_stem(buff_id)
        out = [existing for existing in out if buff_stem(existing) != stem]
        out.append(buff_id)
    return set(out)


def extract_expected(character: dict[str, Any], building: dict[str, Any]) -> dict[str, Any]:
    chars = building.get("chars", {})
    buffs = building.get("buffs", {})
    expected: dict[str, Any] = {}
    for char_id, char in character.items():
        source_char = chars.get(char_id)
        if not source_char:
            continue
        operator = char.get("name", char_id)
        by_facility: dict[str, list[dict[str, Any]]] = defaultdict(list)
        for phase_entry in source_char.get("buffChar", []):
            for buff_data in phase_entry.get("buffData", []):
                buff_id = buff_data.get("buffId")
                if not buff_id:
                    continue
                cond = buff_data.get("cond", {})
                phase = int(cond.get("phase", 0))
                facility = facility_for(buffs.get(buff_id, {}), buff_id)
                by_facility[facility].append(
                    {
                        "buff_id": buff_id,
                        "phase": phase,
                        "level": cond.get("level"),
                    }
                )
        expected[operator] = {
            "source_rarity": int(char.get("rarity", 0)),
            "facilities": dict(by_facility),
        }
    return expected


def audit_instances(
    expected: dict[str, Any],
    instances: dict[str, Any],
    operators: set[str] | None = None,
    provenance: dict[tuple[str, str, str], str] | None = None,
) -> list[dict[str, Any]]:
    findings: list[dict[str, Any]] = []
    provenance = provenance or {}
    modeled_operators = {instance.get("name", key.split("@", 1)[0]) for key, instance in instances.items()}
    for operator, source in expected.items():
        if (
            operator not in modeled_operators
            or source["source_rarity"] <= 0
            or (operators is not None and operator not in operators)
        ):
            continue
        target_phase = runtime_target_phase(source["source_rarity"])
        for facility, rows in source["facilities"].items():
            expected_t0 = latest_phase_rows(rows, 0)
            expected_up = latest_phase_rows(rows, target_phase)
            t0 = instances.get(f"{operator}@tier_0", {}).get("facilities", {}).get(facility, {})
            up = instances.get(f"{operator}@tier_up", {}).get("facilities", {}).get(facility, {})
            source_ids = {row["buff_id"] for row in rows}
            unresolved: list[tuple[str, str]] = []

            def resolve_local(buff_id: str) -> str | None:
                if buff_id in source_ids:
                    return buff_id
                mapped = provenance.get((operator, facility, buff_id))
                if mapped in source_ids:
                    return mapped
                unresolved.append((buff_id, "tier_0"))
                return None

            def resolve_up(buff_id: str) -> str | None:
                if buff_id in source_ids:
                    return buff_id
                mapped = provenance.get((operator, facility, buff_id))
                if mapped in source_ids:
                    return mapped
                unresolved.append((buff_id, "tier_up"))
                return None

            raw_t0 = list(t0.get("buff_ids", []))
            raw_up = list(up.get("buff_ids", []))
            actual_t0 = {mapped for buff_id in raw_t0 if (mapped := resolve_local(buff_id))}
            resolved_up = [mapped for buff_id in raw_up if (mapped := resolve_up(buff_id))]
            actual_up = (
                merge_stepwise(sorted(actual_t0), resolved_up)
                if up.get("stepwise", False)
                else set(resolved_up)
            )
            source_by_id = {row["buff_id"]: row for row in rows}

            for buff_id, tier in sorted(set(unresolved)):
                findings.append(
                    {
                        "kind": "unmapped_local_binding",
                        "operator": operator,
                        "facility": facility,
                        "tier": tier,
                        "local_buff_id": buff_id,
                    }
                )

            def add(kind: str, buff_ids: set[str], tier: str) -> None:
                for buff_id in sorted(buff_ids):
                    row = source_by_id.get(buff_id, {})
                    findings.append(
                        {
                            "kind": kind,
                            "operator": operator,
                            "facility": facility,
                            "tier": tier,
                            "buff_id": buff_id,
                            "source_phase": row.get("phase"),
                            "source_level": row.get("level"),
                            "source_rarity": source["source_rarity"],
                        }
                    )

            add("missing_phase_0", expected_t0 - actual_t0, "tier_0")
            add("missing_tier_up", expected_up - actual_up, "tier_up")
            add("future_unlock_in_tier_0", actual_t0 - expected_t0, "tier_0")
            add("unexpected_or_stale_tier_up", actual_up - expected_up, "tier_up")

    known_by_operator: dict[str, dict[str, str]] = defaultdict(dict)
    for operator, source in expected.items():
        for facility, rows in source["facilities"].items():
            for row in rows:
                known_by_operator[operator][row["buff_id"]] = facility
    for key, instance in instances.items():
        operator = instance.get("name", key.split("@", 1)[0])
        for facility, binding in instance.get("facilities", {}).items():
            for buff_id in binding.get("buff_ids", []):
                expected_facility = known_by_operator.get(operator, {}).get(buff_id)
                if expected_facility and expected_facility != facility:
                    findings.append(
                        {
                            "kind": "wrong_facility_binding",
                            "operator": operator,
                            "facility": facility,
                            "expected_facility": expected_facility,
                            "tier": instance.get("tier"),
                            "buff_id": buff_id,
                        }
                    )
    return findings


def audit_document(
    character: dict[str, Any],
    building: dict[str, Any],
    instances: dict[str, Any],
    operators: set[str] | None = None,
    provenance: dict[tuple[str, str, str], str] | None = None,
) -> list[dict[str, Any]]:
    return audit_instances(extract_expected(character, building), instances, operators, provenance)


def load_provenance(path: Path) -> dict[tuple[str, str, str], str]:
    if not path.exists():
        return {}
    document = json.loads(path.read_text(encoding="utf-8"))
    return {
        (row["operator"], row["facility"], row["local_buff_id"]): row["source_buff_id"]
        for row in document.get("mappings", [])
        if row.get("status") == "accepted"
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, help="directory with character_table.json and building_data.json")
    parser.add_argument("--instances", type=Path, default=ROOT / "data/operator_instances.json")
    parser.add_argument(
        "--provenance",
        type=Path,
        default=ROOT / "data/building_skill_provenance.json",
        help="accepted local-to-upstream buff mappings",
    )
    parser.add_argument("--operator", action="append", dest="operators", help="audit only this operator; repeatable")
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()

    character, building = load_sources(args.source_dir)
    instances = json.loads(args.instances.read_text(encoding="utf-8"))["instances"]
    findings = audit_document(
        character,
        building,
        instances,
        set(args.operators) if args.operators else None,
        load_provenance(args.provenance),
    )
    report = {
        "source": "yuanyan3060/ArknightsGameResource",
        "runtime_states": {"tier_0": "phase_0", "tier_up_3_4_star": "phase_1", "tier_up_5_6_star": "phase_2"},
        "finding_count": len(findings),
        "findings": findings,
    }
    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"building skill unlock audit: findings={len(findings)}")
    for finding in findings[:40]:
        print("  " + json.dumps(finding, ensure_ascii=False, sort_keys=True))
    if len(findings) > 40:
        print(f"  ... +{len(findings) - 40} more; use --json-out for the complete report")
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
