#!/usr/bin/env python3
"""Audit trade buff_ids: missing from skill_table vs PRTS primitive class."""

from __future__ import annotations

import csv
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"
OLD = Path(r"C:\Users\KnightCode\projects\ArknightsInfraCalc - 副本\data")

# One-shot batch: existing atoms only, no new primitives
ONE_SHOT_PRIMS = {
    "ADD_FLAT",
    "LIMIT_DELTA_FLAT",
}
# Pair/limit with existing Condition + atoms
EXISTING_ATOM_PRIMS = {
    "LIMIT_DELTA_FLAT",  # + partner_in_room + mood
    "ADD_FLAT",
}
# Needs dedicated module / new primitive
SPECIAL_PRIMS = {
    "ORDER_MECHANIC",
    "ADD_PAIR_CONDITIONAL",
    "ADD_BASE_CONDITIONAL",
    "ADD_CROSS_FACILITY_CONDITIONAL",
    "ADD_LAYOUT_SCALED",
    "ADD_PER_TAG_IN_ROOM",
    "ADD_PER_ROOM_PEER_CAPITA",
    "ADD_PER_EXCESS_LIMIT",
    "ADD_PER_ORDER_GAP",
    "ADD_PER_ORDER_COUNT",
    "COMPRESS_LIMIT_FROM_PEER_EFF",
    "ADD_PER_PEER_EFF_BUCKET",
    "ADD_PER_LIMIT_CONTRIB_BUCKET",
    "ZERO_PEER_EFF",
    "WRITE_STATE",
    "CUSTOM",
}


def load_primitives() -> dict[str, dict]:
    path = OLD / "PRIMITIVE_COVERAGE.csv"
    out: dict[str, dict] = {}
    if not path.exists():
        return out
    with path.open(encoding="utf-8-sig") as f:
        for row in csv.DictReader(f):
            bid = row["buff_id"]
            if not bid.startswith("trade_"):
                continue
            out[bid] = row
    return out


def main() -> None:
    skills = {s["id"]: s for s in json.loads((DATA / "skill_table.json").read_text())["skills"]}
    inst = json.loads((DATA / "operator_instances.json").read_text())["instances"]
    prts_rows = json.loads((DATA / "prts_trade_skills.json").read_text())["rows"]
    prts_by_name = {r["skill_name"]: r["description"] for r in prts_rows}

    prim = load_primitives()

    all_bids: set[str] = set()
    for i in inst.values():
        t = i.get("facilities", {}).get("trade")
        if t:
            all_bids.update(t["buff_ids"])

    missing = sorted(b for b in all_bids if b not in skills)
    in_table = sorted(all_bids & skills.keys())

    one_shot: list[tuple[str, dict]] = []
    pair_limit_mood: list[tuple[str, dict]] = []
    special: list[tuple[str, dict]] = []
    empty_atoms: list[tuple[str, dict]] = []

    for bid in missing:
        p = prim.get(bid, {})
        pr = p.get("primitive", "?")
        if pr in ONE_SHOT_PRIMS:
            one_shot.append((bid, p))
        elif pr == "LIMIT_DELTA_FLAT" and "cost_P" in bid:
            pair_limit_mood.append((bid, p))
        else:
            special.append((bid, p))

    for bid in in_table:
        atoms = skills[bid].get("atoms", [])
        if not atoms:
            p = prim.get(bid, {})
            empty_atoms.append((bid, p))

    print("=== COVERAGE ===")
    print(f"skill_table entries: {len(skills)}")
    print(f"trade buff_ids in instances: {len(all_bids)}")
    print(f"already in skill_table: {len(in_table)}")
    print(f"missing: {len(missing)}")
    print(f"in table but empty atoms (gold_flow etc): {len(empty_atoms)}")
    print()
    print("=== ONE-SHOT (ADD_FLAT / simple LIMIT+FLAT) ===")
    for bid, p in one_shot:
        print(f"  {bid} | {p.get('skill','')} | {p.get('primitive')}")
    print(f"count: {len(one_shot)}")
    print()
    print("=== PAIR+LIMIT+MOOD (existing partner_in_room) ===")
    for bid, p in pair_limit_mood:
        print(f"  {bid} | {p.get('skill','')} | {p.get('primitive')}")
    print(f"count: {len(pair_limit_mood)}")
    print()
    print("=== SPECIAL (defer) ===")
    for bid, p in special:
        print(f"  {bid} | {p.get('技能名','')} | {p.get('phase')} | {p.get('primitive')}")
    print(f"count: {len(special)}")


if __name__ == "__main__":
    main()
