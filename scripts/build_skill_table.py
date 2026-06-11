#!/usr/bin/env python3
"""校验 skill_table.json 与 operator_instances 引用完整性。"""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKILL_TABLE = ROOT / "data" / "skill_table.json"
INSTANCES = ROOT / "data" / "operator_instances.json"

PILOT_OPS = ("但书", "可露希尔", "孑", "德克萨斯", "拉普兰德", "能天使")


def buff_stem(buff_id: str) -> str:
    return buff_id.rsplit("[", 1)[0] if "[" in buff_id else buff_id


def merge_stepwise(t0: list[str], up: list[str]) -> list[str]:
    if all(x in up for x in t0) and len(up) >= len(t0):
        return list(up)
    out = list(t0)
    for bid in up:
        if bid in out:
            continue
        stem = buff_stem(bid)
        out = [x for x in out if buff_stem(x) != stem]
        out.append(bid)
    return out


def resolve_trade_buff_ids(
    instances: dict, name: str, tier: str
) -> list[str]:
    key = f"{name}@{tier}"
    inst = instances.get(key)
    if not inst:
        return []
    trade = inst.get("facilities", {}).get("trade")
    if not trade:
        return []
    binding_ids = trade["buff_ids"]
    stepwise = trade.get("stepwise", False)
    if tier == "tier_0" or not stepwise:
        return list(binding_ids)
    t0_key = f"{name}@tier_0"
    t0_trade = (
        instances.get(t0_key, {}).get("facilities", {}).get("trade") if t0_key in instances else None
    )
    if not t0_trade:
        return list(binding_ids)
    return merge_stepwise(t0_trade["buff_ids"], binding_ids)


def main() -> int:
    skills = json.loads(SKILL_TABLE.read_text(encoding="utf-8"))
    skill_ids = {s["id"] for s in skills["skills"]}
    instances = json.loads(INSTANCES.read_text(encoding="utf-8"))["instances"]

    legacy = [s["id"] for s in skills["skills"] if s["id"].startswith("skill_")]
    if legacy:
        print(f"FAIL: legacy skill_* ids in skill_table: {legacy}")
        return 1

    pilot_missing: list[str] = []
    for name in PILOT_OPS:
        for tier in ("tier_0", "tier_up"):
            for bid in resolve_trade_buff_ids(instances, name, tier):
                if bid not in skill_ids:
                    pilot_missing.append(f"{name}@{tier}: {bid}")

    all_missing: list[str] = []
    for key, inst in sorted(instances.items()):
        trade = inst.get("facilities", {}).get("trade")
        if not trade:
            continue
        name = inst["name"]
        tier = inst["tier"]
        for bid in resolve_trade_buff_ids(instances, name, tier):
            if bid not in skill_ids:
                all_missing.append(f"{key}: {bid}")

    print(f"skill_table: {len(skills['skills'])} buff defs")
    print(f"instances trade refs missing (all): {len(all_missing)}")
    print(f"instances trade refs missing (pilot): {len(pilot_missing)}")

    if pilot_missing:
        print("\nFAIL pilot operators:")
        for line in pilot_missing:
            print(f"  {line}")
        return 1

    if all_missing:
        print("\nWARN non-pilot missing (expected until 88-skill batch):")
        for line in all_missing[:15]:
            print(f"  {line}")
        if len(all_missing) > 15:
            print(f"  ... and {len(all_missing) - 15} more")

    print("OK: pilot operator buff_ids resolve in skill_table")
    return 0


if __name__ == "__main__":
    sys.exit(main())
