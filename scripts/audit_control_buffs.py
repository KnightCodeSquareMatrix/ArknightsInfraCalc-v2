#!/usr/bin/env python3
"""Audit control_center buff_ids: instances binding vs skill_table atoms."""

from __future__ import annotations

import json
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"
OUT = ROOT / "out" / "audit_control.txt"


def buff_stem(bid: str) -> str:
    return bid.rsplit("[", 1)[0] if "[" in bid else bid


def resolve_buff_ids(tier: str, binding: dict, tier0_binding: dict | None) -> list[str]:
    if tier == "tier_0":
        return list(binding["buff_ids"])
    if not binding.get("stepwise", False):
        return list(binding["buff_ids"])
    if not tier0_binding:
        return list(binding["buff_ids"])
    out = list(tier0_binding["buff_ids"])
    for bid in binding["buff_ids"]:
        if bid in out:
            continue
        stem = buff_stem(bid)
        out = [x for x in out if buff_stem(x) != stem]
        out.append(bid)
    return out


def classify_bid(bid: str, skills: dict) -> str:
    if bid not in skills:
        return "missing_table"
    sk = skills[bid]
    if sk.get("facility") != "control":
        return f"wrong_facility:{sk.get('facility')}"
    if not sk.get("atoms", []):
        return "empty_atoms"
    return "ready"


def main() -> None:
    skills = {
        s["id"]: s
        for s in json.loads((DATA / "skill_table.json").read_text(encoding="utf-8"))["skills"]
    }
    inst = json.loads(
        (DATA / "operator_instances.json").read_text(encoding="utf-8")
    )["instances"]

    rows: list[dict] = []
    ops: dict[str, list[dict]] = {}

    for _key, row in inst.items():
        ctrl = row.get("facilities", {}).get("control")
        if not ctrl:
            continue
        name = row["name"]
        tier = row["tier"]
        t0 = inst.get(f"{name}@tier_0", {}).get("facilities", {}).get("control")
        bids = resolve_buff_ids(tier, ctrl, t0)
        entry = {
            "tier": tier,
            "buff_ids": bids,
            "tags": row.get("tags", []),
        }
        ops.setdefault(name, []).append(entry)

        statuses = [classify_bid(b, skills) for b in bids]
        worst = "ready"
        rank = {"ready": 0, "empty_atoms": 1, "missing_table": 2}
        for s in statuses:
            if s.startswith("wrong_facility"):
                worst = s
                break
            if rank.get(s, 0) > rank.get(worst, 0):
                worst = s
        rows.append(
            {
                "name": name,
                "tier": tier,
                "buff_ids": bids,
                "statuses": statuses,
                "tags": entry["tags"],
                "worst": worst,
                "skill_names": [
                    skills[b].get("skill_name", "?") if b in skills else "?"
                    for b in bids
                ],
            }
        )

    lines: list[str] = []
    append = lines.append

    append("=== CONTROL CENTER AUDIT ===")
    append("")

    c = Counter(r["worst"] for r in rows)
    append("Instance rows (tier_0 + tier_up):")
    for k, v in sorted(c.items(), key=lambda x: -x[1]):
        append(f"  {k}: {v}")

    unique_ops: dict[str, tuple[int, dict]] = {}
    for r in rows:
        name = r["name"]
        if r["worst"] == "ready":
            rank = 0
        elif r["worst"] == "empty_atoms":
            rank = 1
        elif r["worst"] == "missing_table":
            rank = 2
        else:
            rank = 3
        if name not in unique_ops or rank > unique_ops[name][0]:
            unique_ops[name] = (rank, r)

    uc = Counter()
    for rank, r in unique_ops.values():
        if rank == 0:
            uc["ready"] += 1
        elif rank == 1:
            uc["empty_atoms"] += 1
        elif rank == 2:
            uc["missing_table"] += 1
        else:
            uc[r["worst"]] += 1

    append("")
    append(f"Unique operators with control binding: {len(unique_ops)}")
    for k, v in sorted(uc.items(), key=lambda x: -x[1]):
        append(f"  {k}: {k} -> {v}")

    append("")
    append(f"=== READY ({sum(1 for r in rows if r['worst'] == 'ready')}) ===")
    for r in sorted(rows, key=lambda x: (x["name"], x["tier"])):
        if r["worst"] != "ready":
            continue
        append(
            f"  {r['name']} ({r['tier']}): "
            + " | ".join(
                f"{b} ({n})" for b, n in zip(r["buff_ids"], r["skill_names"])
            )
        )

    append("")
    append("=== EMPTY ATOMS (registered, not executable) ===")
    for r in sorted(rows, key=lambda x: (x["name"], x["tier"])):
        if r["worst"] != "empty_atoms":
            continue
        append(f"  {r['name']} ({r['tier']}): {r['buff_ids']}")

    append("")
    append("=== MISSING FROM skill_table ===")
    for r in sorted(rows, key=lambda x: (x["name"], x["tier"])):
        if r["worst"] != "missing_table":
            continue
        append(f"  {r['name']} ({r['tier']}): {r['buff_ids']}")

    append("")
    append("=== WRONG FACILITY ===")
    for r in sorted(rows, key=lambda x: (x["name"], x["tier"])):
        if not r["worst"].startswith("wrong_facility"):
            continue
        append(
            f"  {r['name']} ({r['tier']}): "
            + str(list(zip(r["buff_ids"], r["statuses"])))
        )

    all_ctrl_bids = {
        sk["id"] for sk in skills.values() if sk.get("facility") == "control"
    }
    used = {b for r in rows for b in r["buff_ids"]}
    orphan = sorted(all_ctrl_bids - used)
    append("")
    append("=== skill_table control buffs NOT bound to any instance ===")
    for b in orphan:
        append(f"  {b}: {skills[b].get('skill_name')}")

    all_used = sorted({b for r in rows for b in r["buff_ids"]})
    append("")
    append(f"Distinct control buff_ids in instances: {len(all_used)}")
    append(f"Control entries in skill_table: {len(all_ctrl_bids)}")
    append("")
    append("All instance buff_ids:")
    for b in all_used:
        st = classify_bid(b, skills)
        sn = skills[b].get("skill_name", "?") if b in skills else "?"
        append(f"  {b} [{st}] {sn}")

    # --- operator buckets (pool readiness) ---
    MOOD_STEMS = {
        "control_mp_cost",
        "control_mp_cost&faction",
        "control_mp_cost&faction2",
        "control_dorm_rec",
        "control_dorm_rec2",
        "control_dorm_rec_tag",
        "control_facCostReset",
        "control_hire_spd&bd",
        "control_mp_cost_reset",
        "control_mp_cost_double",
        "control_mp_expand_double",
        "control_mp_lonely",
        "control_mp_psk",
        "control_allCost_condChar",
        "control_mp_aegir1",
        "control_mp_aegir2",
        "control_mp_bd_cost_expand",
    }

    def categorize_bid(bid: str) -> str:
        if bid in skills and skills[bid].get("atoms"):
            return "ready"
        st = buff_stem(bid)
        if st in MOOD_STEMS or st.startswith("control_mp_cost"):
            return "mood_non_goal"
        return "efficiency_candidate"

    op_summary: list[tuple[str, str, list[str], list[str]]] = []
    for name in sorted(ops):
        tiers: list[tuple[str, list[str], list[str]]] = []
        for entry in ops[name]:
            bids = entry["buff_ids"]
            cats = [categorize_bid(b) for b in bids]
            tiers.append((entry["tier"], bids, cats))
        all_ready = all(all(c == "ready" for c in cats) for _, _, cats in tiers)
        missing_eff = sorted(
            {b for _, bids, cats in tiers for b, c in zip(bids, cats) if c == "efficiency_candidate"}
        )
        missing_mood = sorted(
            {b for _, bids, cats in tiers for b, c in zip(bids, cats) if c == "mood_non_goal"}
        )
        if all_ready:
            bucket = "READY"
        elif missing_eff and not missing_mood:
            bucket = "NEEDS_EFF_ONLY"
        elif missing_eff and missing_mood:
            bucket = "BLOCKED_MOOD_AND_EFF"
        elif missing_mood:
            bucket = "MOOD_ONLY_NON_GOAL"
        else:
            bucket = "OTHER"
        op_summary.append((name, bucket, missing_eff, missing_mood))

    append("")
    append("=== OPERATOR BUCKETS (pool entry) ===")
    bc = Counter(s[1] for s in op_summary)
    for k, v in bc.most_common():
        append(f"  {k}: {v}")

    append("")
    append("=== NEEDS_EFF_ONLY (add atoms → pool ready) ===")
    for name, bucket, eff, _ in op_summary:
        if bucket != "NEEDS_EFF_ONLY":
            continue
        append(f"  {name}: {eff}")

    append("")
    append("=== BLOCKED_MOOD_AND_EFF (eff in table but mood buff blocks pool) ===")
    for name, bucket, eff, mood in op_summary:
        if bucket != "BLOCKED_MOOD_AND_EFF":
            continue
        eff_ready = [b for b in eff if b in skills and skills[b].get("atoms")]
        append(f"  {name}:")
        append(f"    eff missing: {eff}")
        append(f"    mood block: {mood}")
        if eff_ready:
            append(f"    eff already tabled: {eff_ready}")

    append("")
    append("=== MOOD_ONLY_NON_GOAL ===")
    for name, bucket, _, mood in op_summary:
        if bucket != "MOOD_ONLY_NON_GOAL":
            continue
        append(f"  {name}: {mood}")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(OUT.read_text(encoding="utf-8"))


if __name__ == "__main__":
    main()
