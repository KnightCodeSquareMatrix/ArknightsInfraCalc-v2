#!/usr/bin/env python3
"""Plain set-diff check: PRTS operators vs instances vs skill_table. No regex."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"


def split_ops(text: str) -> list[str]:
    return [part.strip() for part in text.split("；") if part.strip()]


def base_name(key: str) -> str:
    return key.split("@", 1)[0] if "@" in key else key


def load_roster(path: Path) -> set[str]:
    lines = path.read_text(encoding="utf-8-sig").strip().splitlines()
    if len(lines) < 2:
        return set()
    header = lines[0].split(",")
    name_idx = header.index("name")
    return {line.split(",")[name_idx].strip() for line in lines[1:] if line.strip()}


def main() -> None:
    prts = json.loads((DATA / "prts_trade_skills.json").read_text(encoding="utf-8"))
    prts_ops: set[str] = set()
    prts_by_op: dict[str, list[str]] = {}
    for row in prts["rows"]:
        for op in split_ops(row["operators"]):
            prts_ops.add(op)
            prts_by_op.setdefault(op, []).append(row["skill_name"])

    inst = json.loads((DATA / "operator_instances.json").read_text(encoding="utf-8"))["instances"]
    inst_trade: dict[str, list[str]] = {}
    inst_by_op: dict[str, list[str]] = {}
    all_bids: set[str] = set()
    for name, rec in inst.items():
        trade = rec.get("facilities", {}).get("trade")
        if not trade:
            continue
        bids = trade.get("buff_ids", [])
        inst_trade[name] = bids
        inst_by_op.setdefault(base_name(name), []).extend(bids)
        all_bids.update(bids)

    skills = {
        s["id"]: s
        for s in json.loads((DATA / "skill_table.json").read_text(encoding="utf-8"))["skills"]
    }
    trade_skills = {k: v for k, v in skills.items() if k.startswith("trade_")}

    roster = load_roster(DATA / "roster.csv")
    roster_g = load_roster(DATA / "roster_gongsun.csv")

    inst_ops = set(inst_by_op.keys())

    print("=== 人数 ===")
    print(f"PRTS 贸易技能干员（去重）: {len(prts_ops)}")
    print(f"operator_instances 有 trade（去重）: {len(inst_ops)}")
    print(f"operator_instances trade 条目（含 tier）: {len(inst_trade)}")
    print(f"roster.csv: {len(roster)}")
    print(f"roster_gongsun.csv: {len(roster_g)}")
    print()

    missing_in_inst = sorted(prts_ops - inst_ops)
    extra_in_inst = sorted(inst_ops - prts_ops)

    print("=== PRTS 有、instances 没有（可能少人）===")
    if not missing_in_inst:
        print("  （无）")
    else:
        for op in missing_in_inst:
            skill_names = "、".join(prts_by_op.get(op, []))
            print(f"  {op}  | 技能: {skill_names}")
    print(f"count: {len(missing_in_inst)}")
    print()

    print("=== instances 有、PRTS 没有 ===")
    if not extra_in_inst:
        print("  （无）")
    else:
        for op in extra_in_inst:
            print(f"  {op}  | buff_ids: {sorted(set(inst_by_op[op]))}")
    print(f"count: {len(extra_in_inst)}")
    print()

    missing_bids = sorted(b for b in all_bids if b not in trade_skills)
    in_table = sorted(b for b in all_bids if b in trade_skills)
    empty_atoms = sorted(b for b in in_table if not trade_skills[b].get("atoms"))

    print("=== buff_id 覆盖 ===")
    print(f"instances 引用: {len(all_bids)}")
    print(f"skill_table 已有: {len(in_table)}")
    print(f"skill_table 缺失: {len(missing_bids)}")
    print(f"在表但 atoms 为空: {len(empty_atoms)}")
    print()

    def owners_of(bid: str) -> list[str]:
        return sorted({base_name(n) for n, bids in inst_trade.items() if bid in bids})

    print("缺失 buff_id（附干员）:")
    for bid in missing_bids:
        print(f"  {bid}  <- {owners_of(bid)}")

    print()
    print("空 atoms（附干员）:")
    for bid in empty_atoms:
        print(f"  {bid}  <- {owners_of(bid)}")

    print()
    print("=== roster 与 instances 差异 ===")
    print(f"roster.csv 不在 instances.trade: {sorted(roster - inst_ops) or '（无）'}")
    print(f"instances.trade 不在 roster.csv: {sorted(inst_ops - roster) or '（无）'}")
    print(f"roster_gongsun 不在 instances.trade: {sorted(roster_g - inst_ops) or '（无）'}")
    print(f"instances.trade 不在 roster_gongsun: {sorted(inst_ops - roster_g) or '（无）'}")


if __name__ == "__main__":
    main()
