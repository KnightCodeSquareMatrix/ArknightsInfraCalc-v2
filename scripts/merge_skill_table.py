#!/usr/bin/env python3
"""Merge trade_skill_table_0/up buff_defs into skill_table.json, then remove split files."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"

TIER_UP_ONLY = {
    "trade_ord_spd&limit[001]",
    "trade_ord_spd&limit[021]",
    "trade_ord_spd&limit[022]",
    "trade_ord_spd&limit[031]",
    "trade_ord_spd&limit[033]",
    "trade_ord_spd&limit[035]",
    "trade_ord_spd&limit[101]",
    "trade_ord_spd[011]",
    "trade_ord_spd[020]",
    "trade_ord_spd[021]",
    "trade_ord_spd[1001]",
}


def load_buff_defs() -> dict[str, dict]:
    by_id: dict[str, dict] = {}
    in_t0: set[str] = set()
    in_up: set[str] = set()
    for name, tier_file in (
        ("trade_skill_table_0.json", "tier_0"),
        ("trade_skill_table_up.json", "tier_up"),
    ):
        path = DATA / name
        if not path.exists():
            raise SystemExit(f"missing {path}")
        for buff in json.loads(path.read_text(encoding="utf-8"))["buff_defs"]:
            bid = buff["id"]
            by_id[bid] = buff
            if tier_file == "tier_0":
                in_t0.add(bid)
            else:
                in_up.add(bid)
    if len(by_id) != 26:
        raise SystemExit(f"expected 26 paper buffs, got {len(by_id)}")
    tiers = {}
    for bid in by_id:
        if bid in TIER_UP_ONLY:
            tiers[bid] = "tier_up"
        else:
            tiers[bid] = "tier_0"
    return by_id, tiers


def main() -> None:
    skill_table = json.loads((DATA / "skill_table.json").read_text(encoding="utf-8"))
    buff_defs, tiers = load_buff_defs()

    paper_skills = []
    for bid in sorted(buff_defs):
        b = buff_defs[bid]
        paper_skills.append(
            {
                "id": bid,
                "skill_name": b["skill_name"],
                "facility": b["facility"],
                "tier": tiers[bid],
                "atoms": b["atoms"],
            }
        )

    skill_table["skills"] = skill_table["skills"] + paper_skills
    (DATA / "skill_table.json").write_text(
        json.dumps(skill_table, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    for name in ("trade_skill_table_0.json", "trade_skill_table_up.json"):
        path = DATA / name
        if path.exists():
            path.unlink()
            print(f"removed {path.name}")

    print(f"skill_table.json: {len(skill_table['skills'])} skills ({len(paper_skills)} paper + complex)")


if __name__ == "__main__":
    main()
