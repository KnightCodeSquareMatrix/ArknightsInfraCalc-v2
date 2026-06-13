#!/usr/bin/env python3
"""Audit trade operators whose E2 skill name differs from E0 (PRTS / MECHANICS_REGISTRY)."""
import csv
import json
import re
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def split_ops(s: str) -> list[str]:
    return [x.strip() for x in re.split(r"[;；]", s) if x.strip()]


def is_alpha_beta(a: str, b: str) -> bool:
    return a.endswith("·α") and b.endswith("·β") and a[:-2] == b[:-2]


def main() -> None:
    by_op: dict[str, dict[str, list[dict]]] = defaultdict(
        lambda: {"精0": [], "精1": [], "精2": []}
    )
    with open(ROOT / "data/MECHANICS_REGISTRY.csv", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            if row["工作设施"] != "贸易站":
                continue
            elite = row["需求精英"].strip()
            if elite not in ("精0", "精1", "精2"):
                continue
            ops = split_ops(row["干员"])
            if not ops:
                continue
            entry = {
                "skill": row["技能名"].strip(),
                "eff": row["效率值"].strip(),
                "desc": row["游戏原文"],
            }
            for op in ops:
                by_op[op][elite].append(entry)

    with open(ROOT / "data/operator_instances.json", encoding="utf-8") as f:
        instances = json.load(f)["instances"]

    def inst_trade(op: str, tier: str) -> dict | None:
        key = f"{op}@{tier}"
        inst = instances.get(key)
        if not inst:
            return None
        return inst.get("facilities", {}).get("trade")

    pure_replace = []
    rename_stepwise = []
    alpha_beta = []
    mood_only = []

    for op in sorted(by_op):
        e0 = by_op[op]["精0"]
        e2 = by_op[op]["精2"]
        if not e0 or not e2:
            continue
        e0_names = list(dict.fromkeys(x["skill"] for x in e0))
        e2_names = list(dict.fromkeys(x["skill"] for x in e2))
        if e0_names == e2_names:
            continue

        tu = inst_trade(op, "tier_up")
        t0 = inst_trade(op, "tier_0")
        stepwise = tu.get("stepwise") if tu else None
        buff_up = tu.get("buff_ids", []) if tu else []
        buff_0 = t0.get("buff_ids", []) if t0 else []

        if len(e0_names) == 1 and len(e2_names) == 1:
            a, b = e0_names[0], e2_names[0]
            if is_alpha_beta(a, b):
                alpha_beta.append((op, a, b))
                continue
            row = (op, a, b, e0[0]["eff"], e2[0]["eff"], stepwise, buff_0, buff_up)
            # mood skills often have empty eff in registry
            if not e0[0]["eff"] and not e2[0]["eff"]:
                mood_only.append(row)
            elif stepwise is False and len(buff_up) == 1:
                pure_replace.append(row)
            else:
                rename_stepwise.append(row)

    print("=== 银灰型：精2整槽替换（stepwise=false，tier_up 仅新 buff）===")
    for op, a, b, e0e, e2e, sw, b0, bu in pure_replace:
        print(f"  {op}: {a} -> {b}  | eff {e0e or '-'} -> {e2e or '-'}  | buff {b0} -> {bu}")

    print(f"\n共 {len(pure_replace)} 人")

    print("\n=== 换名但精2叠层（stepwise=true，像能天使/可颂）===")
    for op, a, b, e0e, e2e, sw, b0, bu in rename_stepwise:
        print(f"  {op}: {a} -> {b}  | eff {e0e or '-'} -> {e2e or '-'}  | buff {b0} -> {bu}")

    print(f"\n共 {len(rename_stepwise)} 人")

    print("\n=== 换名但多为心情技（registry 无效率列）===")
    for op, a, b, e0e, e2e, sw, b0, bu in mood_only:
        print(f"  {op}: {a} -> {b}  | stepwise={sw}  | buff {b0} -> {bu}")

    print(f"\n共 {len(mood_only)} 人")

    print("\n=== α→β 同系列（非整名替换）===")
    for op, a, b in alpha_beta:
        print(f"  {op}: {a} -> {b}")
    print(f"\n共 {len(alpha_beta)} 人")


if __name__ == "__main__":
    main()
