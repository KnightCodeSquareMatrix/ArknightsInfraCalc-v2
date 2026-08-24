#!/usr/bin/env python3
"""Sync building-skill metadata from ArknightsGameResource.

The upstream repository is the raw source for operator/building skill facts.
This script updates metadata only; it never invents EffectAtom definitions.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"
UPSTREAM = "https://raw.githubusercontent.com/yuanyan3060/ArknightsGameResource/main"

SOURCE_FILES = {
    "character_table": "gamedata/excel/character_table.json",
    "building_data": "gamedata/excel/building_data.json",
    "data_version": "gamedata/excel/data_version.txt",
}

CSV_FIELDS = [
    "source_repo",
    "source_version",
    "char_id",
    "operator",
    "tier",
    "facility",
    "buff_id",
    "skill_name",
    "description",
    "unlock_phase",
    "unlock_level",
]


def fetch_json(url: str) -> object:
    request = urllib.request.Request(url, headers={"User-Agent": "ArknightsInfraCalc-sync/1"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def fetch_text(url: str) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": "ArknightsInfraCalc-sync/1"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read().decode("utf-8")


def load_sources(source_dir: Path | None) -> tuple[dict, dict, str]:
    if source_dir is not None:
        character = json.loads((source_dir / "character_table.json").read_text(encoding="utf-8"))
        building = json.loads((source_dir / "building_data.json").read_text(encoding="utf-8"))
        version = (source_dir / "data_version.txt").read_text(encoding="utf-8").strip().replace("\n", " | ")
        return character, building, version
    return (
        fetch_json(f"{UPSTREAM}/{SOURCE_FILES['character_table']}"),
        fetch_json(f"{UPSTREAM}/{SOURCE_FILES['building_data']}"),
        fetch_text(f"{UPSTREAM}/{SOURCE_FILES['data_version']}").strip().replace("\n", " | "),
    )


def tier_for_phase(phase: int) -> str:
    if phase == 0:
        return "tier_0"
    if phase == 2:
        return "tier_up"
    return f"phase_{phase}"


def facility_for_room_type(room_type: str | None, buff_id: str) -> str:
    if room_type:
        return {
            "CONTROL": "control",
            "MANUFACT": "manufacture",
            "TRADE": "trade",
            "POWER": "power",
            "DORMITORY": "dorm",
            "HIRE": "office",
            "MEETING": "meeting",
            "TRAINING": "training",
            "WORKSHOP": "workshop",
        }.get(room_type, room_type.lower())
    prefix = buff_id.split("_", 1)[0]
    return {"control": "control", "trade": "trade", "manu": "manufacture", "power": "power"}.get(
        prefix, "unknown"
    )


def extract_rows(character: dict, building: dict, version: str) -> list[dict[str, str]]:
    chars = building.get("chars", {})
    buffs = building.get("buffs", {})
    rows: dict[tuple[str, str, str, str], dict[str, str]] = {}
    for char_id, char in character.items():
        source_char = chars.get(char_id)
        if not source_char:
            continue
        operator = char.get("name", char_id)
        for phase_entry in source_char.get("buffChar", []):
            for buff_data in phase_entry.get("buffData", []):
                buff_id = buff_data.get("buffId")
                if not buff_id:
                    continue
                buff = buffs.get(buff_id, {})
                cond = buff_data.get("cond", {})
                phase = int(cond.get("phase", 0))
                tier = tier_for_phase(phase)
                key = (operator, tier, buff_id, facility_for_room_type(buff.get("roomType"), buff_id))
                rows[key] = {
                    "source_repo": "yuanyan3060/ArknightsGameResource",
                    "source_version": version,
                    "char_id": char_id,
                    "operator": operator,
                    "tier": tier,
                    "facility": facility_for_room_type(buff.get("roomType"), buff_id),
                    "buff_id": buff_id,
                    "skill_name": buff.get("buffName", ""),
                    "description": buff.get("description", ""),
                    "unlock_phase": str(phase),
                    "unlock_level": str(cond.get("level", "")),
                }
    return sorted(
        rows.values(), key=lambda row: (row["facility"], row["operator"], row["tier"], row["buff_id"])
    )


def write_csv(path: Path, rows: list[dict[str, str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=CSV_FIELDS)
        writer.writeheader()
        writer.writerows(rows)


def update_skill_table(path: Path, rows: list[dict[str, str]], add_missing: bool) -> tuple[int, int]:
    document = json.loads(path.read_text(encoding="utf-8"))
    skills = {skill["id"]: skill for skill in document["skills"]}
    updated = 0
    added = 0
    for row in rows:
        skill = skills.get(row["buff_id"])
        if skill is None:
            if not add_missing:
                continue
            skill = {
                "id": row["buff_id"],
                "skill_name": row["skill_name"],
                "facility": row["facility"],
                "tier": row["tier"],
                "atoms": [],
            }
            document["skills"].append(skill)
            skills[row["buff_id"]] = skill
            added += 1
            continue
        changed = False
        for field in ("skill_name", "facility", "tier"):
            if row[field] and skill.get(field) != row[field]:
                skill[field] = row[field]
                changed = True
        updated += int(changed)
    document["skills"].sort(key=lambda skill: skill["id"])
    path.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return updated, added


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, help="local directory containing the three upstream files")
    parser.add_argument("--csv", type=Path, default=DATA / "prts_building_skills.csv")
    parser.add_argument("--skill-table", type=Path, default=DATA / "skill_table.json")
    parser.add_argument("--update-skill-table", action="store_true")
    parser.add_argument("--add-missing", action="store_true", help="add missing entries with atoms: []")
    args = parser.parse_args()
    if args.add_missing and not args.update_skill_table:
        parser.error("--add-missing requires --update-skill-table")
    try:
        character, building, version = load_sources(args.source_dir)
        rows = extract_rows(character, building, version)
        write_csv(args.csv, rows)
        updated = added = 0
        if args.update_skill_table:
            updated, added = update_skill_table(args.skill_table, rows, args.add_missing)
    except (OSError, ValueError, KeyError, urllib.error.URLError) as error:
        print(f"sync failed: {error}", file=sys.stderr)
        return 1
    print(f"source_version={version}")
    print(f"rows={len(rows)} csv={args.csv}")
    if args.update_skill_table:
        print(f"skill_table_metadata_updated={updated} added={added}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
