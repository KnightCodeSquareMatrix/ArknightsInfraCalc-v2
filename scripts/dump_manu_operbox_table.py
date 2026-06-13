#!/usr/bin/env python3
"""Dump manufacture pool + triple search for an operbox (L1, 2 gold + 2 exp lines)."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from itertools import combinations
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LAYOUT = {
    "trade_station_count": 3,
    "power_station_count": 3,
    "manu_recipe_kinds": 4,
    "facility_level": 3,
    "mood": 24.0,
}


def tier_key(elite: int) -> str:
    return "tier_up" if elite >= 1 else "tier_0"


def load_data():
    operbox_path = ROOT / "data" / "operbox_xlsx_new.json"
    operbox = {
        e["name"]: e
        for e in json.loads(operbox_path.read_text(encoding="utf-8"))
        if e["own"]
    }
    instances = json.loads(
        (ROOT / "data" / "operator_instances.json").read_text(encoding="utf-8")
    )["instances"]
    skills = {
        s["id"]: s
        for s in json.loads(
            (ROOT / "data" / "skill_table.json").read_text(encoding="utf-8")
        )["skills"]
    }
    return operbox, instances, skills


def resolve_buffs(name: str, elite: int, instances: dict) -> list[str] | None:
    inst = instances.get(f"{name}@{tier_key(elite)}")
    if not inst or "manufacture" not in inst.get("facilities", {}):
        return None
    manu = inst["facilities"]["manufacture"]
    bids = list(manu["buff_ids"])
    if manu.get("stepwise") and elite >= 1:
        t0 = instances.get(f"{name}@tier_0", {}).get("facilities", {}).get("manufacture")
        if t0:
            stems = {b.rsplit("[", 1)[0] for b in bids}
            bids = [b for b in t0["buff_ids"] if b.rsplit("[", 1)[0] not in stems] + bids
    return bids


def sel_value(key: str | None, peer_count: int = 0) -> float:
    if key == "room_peer_count":
        return float(peer_count)
    if key is None:
        return 0.0
    return float(LAYOUT.get(key, 0))


def eval_room(bids_list: list[list[str]], recipe: str, skills: dict):
    base = len(bids_list)
    all_eff = rec_eff = 0.0
    storage = 20
    l2: list[str] = []
    for bids in bids_list:
        peer = len(bids_list) - 1
        for bid in bids:
            sk = skills.get(bid)
            if not sk or sk.get("facility") != "manufacture":
                continue
            atoms = sk.get("atoms") or []
            if not atoms:
                l2.append(bid)
                continue
            for atom in atoms:
                cond = atom.get("condition")
                if cond:
                    ar = cond.get("active_recipe", {})
                    if ar and ar.get("kind") != recipe:
                        continue
                act = atom.get("action", {})
                kind = act.get("kind")
                p = act.get("params", {})
                phase = atom.get("phase")
                if phase == "constant":
                    if kind == "add_flat_eff":
                        v = float(p.get("value", 0))
                        r = p.get("recipe")
                        if r is None:
                            all_eff += v
                        elif r == recipe:
                            rec_eff += v
                    elif kind == "add_flat_eff_from_selector":
                        mult = float(p.get("multiplier", 0))
                        cap = p.get("cap")
                        v = sel_value(atom.get("selector"), peer) * mult
                        if cap is not None:
                            v = min(v, float(cap))
                        r = p.get("recipe")
                        if r is None:
                            all_eff += v
                        elif r == recipe:
                            rec_eff += v
                elif phase == "limit":
                    r = p.get("recipe")
                    if kind == "add_limit_delta" and r in (None, recipe):
                        storage += int(p.get("delta", 0))
    prod = base + all_eff + rec_eff
    return prod, storage, l2


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--top", type=int, default=40)
    args = parser.parse_args()

    operbox, instances, skills = load_data()
    pool = []
    for name, e in operbox.items():
        bids = resolve_buffs(name, e["elite"], instances)
        if not bids:
            continue
        if any(b not in skills for b in bids):
            continue
        g, _, l2g = eval_room([bids], "gold", skills)
        b, _, l2b = eval_room([bids], "battle_record", skills)
        pool.append(
            {
                "name": name,
                "elite": e["elite"],
                "bids": bids,
                "solo_g": g,
                "solo_b": b,
                "solo_comp": 2 * g + 2 * b,
                "l2": sorted(set(l2g + l2b)),
            }
        )
    pool.sort(key=lambda x: (-x["solo_comp"], x["name"]))

    print("=== 制造池 L1 单人贡献 (1 人占 1 位, composite=2×金+2×经验) ===")
    print(
        f"{'#':>3}  {'干员':<8} {'练度':>4}  {'金%':>5}  {'经验%':>6}  "
        f"{'composite':>9}  L2未建模"
    )
    for i, p in enumerate(pool, 1):
        l2 = ",".join(p["l2"][:2]) + ("..." if len(p["l2"]) > 2 else "") if p["l2"] else "-"
        print(
            f"{i:3d}  {p['name']:<8} e{p['elite']}  "
            f"{p['solo_g']:5.0f}  {p['solo_b']:6.0f}  {p['solo_comp']:9.0f}  {l2}"
        )

    hits = []
    for idxs in combinations(range(len(pool)), 3):
        ops = [pool[i] for i in idxs]
        bids_list = [o["bids"] for o in ops]
        g, sg, _ = eval_room(bids_list, "gold", skills)
        b, sb, _ = eval_room(bids_list, "battle_record", skills)
        comp = 2 * g + 2 * b
        hits.append((comp, g, b, sg, sb, [o["name"] for o in ops]))

    hits.sort(key=lambda x: (-x[0], -max(x[4], x[3]), x[5]))

    print()
    print("=== Top 三人组 (4 线 = 2 金 + 2 经验, Lv3, search_baseline layout) ===")
    print(
        f"{'#':>3}  {'composite':>9}  {'金/站%':>6}  {'经验/站%':>8}  "
        f"{'仓金':>4}  {'仓经':>4}  干员"
    )
    for i, (comp, g, b, sg, sb, names) in enumerate(hits[: args.top], 1):
        print(
            f"{i:3d}  {comp:9.0f}  {g:6.0f}  {b:8.0f}  "
            f"{sg:4.0f}  {sb:4.0f}  {names}"
        )

    tier_counts = Counter(h[0] for h in hits)
    print()
    print("=== composite 分档 (全部 C(43,3)=12341 组) ===")
    for score, cnt in sorted(tier_counts.items(), reverse=True)[:8]:
        print(f"  composite={score:.0f}: {cnt} 组")

    top_score = hits[0][0]
    top_hits = [h for h in hits if h[0] == top_score]
    with_caidu = sum(1 for h in top_hits if "裁度" in h[5])
    print()
    print(f"最高分 {top_score:.0f} 共 {len(top_hits)} 组; 含「裁度」{with_caidu} 组")
    flex = Counter()
    for h in top_hits:
        for n in h[5]:
            if n != "酒神":
                flex[n] += 1
    print("最高分组合中「酒神」以外的干员出现次数 (可互换工具位):")
    for n, c in flex.most_common(15):
        print(f"  {n}: {c}/{len(top_hits)}")


if __name__ == "__main__":
    main()
