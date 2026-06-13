#!/usr/bin/env python3
"""Rank owned operators by solo gold / battle_record contribution."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LAYOUT = {"trade_station_count": 3}


def should_tier_up(elite: int, rarity: int, level: int) -> bool:
    if rarity <= 2:
        return level >= 30
    if rarity <= 4:
        return elite >= 1
    return elite >= 2


def load():
    operbox = {
        e["name"]: e
        for e in json.loads((ROOT / "data" / "operbox_xlsx_new.json").read_text(encoding="utf-8"))
        if e["own"]
    }
    instances = json.loads(
        (ROOT / "data" / "operator_instances.json").read_text(encoding="utf-8")
    )["instances"]
    skills = {
        s["id"]: s
        for s in json.loads((ROOT / "data" / "skill_table.json").read_text(encoding="utf-8"))[
            "skills"
        ]
    }
    return operbox, instances, skills


def resolve(name: str, e: dict, instances: dict):
    tier = "tier_up" if should_tier_up(e["elite"], e.get("rarity", 6), e.get("level", 1)) else "tier_0"
    inst = instances.get(f"{name}@{tier}")
    if not inst or "manufacture" not in inst.get("facilities", {}):
        return None
    bids = list(inst["facilities"]["manufacture"]["buff_ids"])
    manu = inst["facilities"]["manufacture"]
    if manu.get("stepwise"):
        t0 = instances.get(f"{name}@tier_0", {}).get("facilities", {}).get("manufacture")
        if t0:
            stems = {b.rsplit("[", 1)[0] for b in bids}
            bids = [b for b in t0["buff_ids"] if b.rsplit("[", 1)[0] not in stems] + bids
    return tier, bids


def solo(recipe: str, bids: list[str], skills: dict):
    eff = 0.0
    l2: list[str] = []
    for bid in bids:
        sk = skills.get(bid)
        if not sk or sk.get("facility") != "manufacture":
            continue
        atoms = sk.get("atoms") or []
        if not atoms:
            l2.append(bid)
            continue
        for atom in atoms:
            if atom.get("phase") != "constant":
                continue
            act = atom.get("action", {})
            p = act.get("params", {})
            kind = act.get("kind")
            if kind == "add_flat_eff":
                r = p.get("recipe")
                v = float(p.get("value", 0))
                if r is None or r == recipe:
                    eff += v
            elif kind == "add_flat_eff_from_selector":
                r = p.get("recipe")
                mult = float(p.get("multiplier", 0))
                sel = atom.get("selector")
                base = LAYOUT.get(sel, 0) if sel else 0
                v = base * mult
                if r is None or r == recipe:
                    eff += v
    return 1 + eff, l2


def main() -> None:
    operbox, instances, skills = load()
    rows = []
    for name, e in operbox.items():
        resolved = resolve(name, e, instances)
        if not resolved:
            continue
        tier, bids = resolved
        if any(b not in skills for b in bids):
            continue
        g, l2g = solo("gold", bids, skills)
        b, l2b = solo("battle_record", bids, skills)
        rows.append(
            (
                g,
                b,
                2 * g + 2 * b,
                name,
                f"e{e['elite']} r{e.get('rarity', 0)} {tier}",
                sorted(set(l2g + l2b)),
            )
        )

    print("=== TOP solo gold (owned, correct tier) ===")
    for g, br, comp, name, meta, l2 in sorted(rows, key=lambda x: -x[0])[:15]:
        l2s = ",".join(l2[:2]) if l2 else "-"
        print(f"{g:5.0f}  {name:<8} {meta}  L2={l2s}")

    print()
    print("=== TOP solo battle_record ===")
    for g, br, comp, name, meta, l2 in sorted(rows, key=lambda x: -x[1])[:15]:
        l2s = ",".join(l2[:2]) if l2 else "-"
        print(f"{br:5.0f}  {name:<8} {meta}  L2={l2s}")

    print()
    watch = ["食铁兽", "帕拉斯", "槐琥", "刻俄柏", "稀音", "至简", "菲亚梅塔", "温蒂", "红云"]
    missing = [n for n in watch if n in operbox and resolve(n, operbox[n], instances) is None]
    print("Known strong ops NOT in manufacture pool:", missing)
    for name in missing:
        print(" ", name, operbox[name])


if __name__ == "__main__":
    main()
