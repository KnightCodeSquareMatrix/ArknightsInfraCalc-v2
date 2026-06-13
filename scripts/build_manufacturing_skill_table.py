#!/usr/bin/env python3
"""Generate manufacture constant-skill entries for skill_table.json."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"

# L2 / deferred: empty atoms until domain engines land.
L2_DELEGATE = {
    "manu_bd_to_bd[000]",
    "manu_cost_all[000]",
    "manu_prod_spd_addition&cost[000]",
    "manu_prod_spd_bd[400]",
    "manu_prod_spd_variable2[000]",
    "manu_prod_spd_variable3[000]",
    "manu_prod_spd_variable[000]",
    "manu_formula_spd&bd[000]",
    "manu_formula_spd&bd[001]",
    "manu_formula_spd&cost[000]",
    "manu_formula_spd&cost[001]",
    "manu_formula_spd&cost_bd[000]",
    "manu_formula_spd&cost_bd[100]",
    "manu_formula_spd&dorm&lv[000]",
}

# buff_id -> (skill_name, tier) when auto-match fails.
MANUAL_SKILL: dict[str, tuple[str, str]] = {
    "manu_formula_spd&limit&cost[000]": ("镜中影", "tier_0"),
    "manu_formula_spd&limit&cost[010]": ("戏中人", "tier_up"),
    "manu_formula_spd&limit&cost[100]": ("“连轴转”", "tier_up"),
    "manu_prod_spd&limit&bd[000]": ("可靠的随从们", "tier_0"),
    "manu_prod_limit&cost[012]": ("午休好去处", "tier_0"),
    "manu_prod_spd_double[100]": ("盛餐的回报", "tier_up"),
    "manu_prod_spd_double[000]": ("“搭把手！”", "tier_up"),
    "manu_constrLv[000]": ("绘图设计", "tier_0"),
    "manu_prod_spd_bd[100]": ("机械辅助·α", "tier_0"),
    "manu_prod_spd_bd[110]": ("机械辅助·β", "tier_up"),
    "manu_prod_spd&fraction[000]": ("重聚时光", "tier_up"),
    "manu_formula_spd_P[000]": ("患难拍档", "tier_up"),
    "manu_skill_spd1[000]": ("意识协议", "tier_0"),
    "manu_skill_spd1[010]": ("源石技艺理论应用", "tier_0"),
    "manu_skill_limit[000]": ("勘探背包", "tier_0"),
    "manu_skill_change[000]": ("意识兼容", "tier_0"),
    "manu_token_prod_spd[010]": ("机械精通·β", "tier_up"),
    "manu_formula_spd[102]": ("原质塑金副产物", "tier_up"),
    "manu_prod_spd&trade[1000]": ("原质塑金副产物", "tier_up"),
    "manu_formula_limit[020]": ("合理利用", "tier_up"),
    "manu_prod_limit&cost[003]": ("智慧之境", "tier_0"),
    "manu_formula_spd[000]": ("胜利之计", "tier_0"),
    "manu_prod_spd&limit&cost[100]": ("得心应手", "tier_0"),
    "manu_prod_spd&limit&cost[200]": ("量体裁衣", "tier_0"),
    "manu_prod_spd&limit&cost[101]": ("虔信", "tier_0"),
    "manu_prod_spd&limit&cost[110]": ("独当一面", "tier_up"),
    "manu_prod_limit&cost[002]": ("“都想要”", "tier_0"),
    "manu_prod_spd_addition[031]": ("“等不及”", "tier_up"),
    "manu_formula_spd[031]": ("公证所教习·β", "tier_up"),
    "manu_formula_spd[030]": ("公证所教习·α", "tier_0"),
    "manu_prod_limit&cost[001]": ("磐蟹·阿盘", "tier_0"),
    "manu_prod_spd[003]": ("磐蟹·豆豆", "tier_0"),
    "manu_prod_spd[004]": ("差遣使魔·α", "tier_0"),
    "manu_prod_spd[014]": ("差遣使魔·β", "tier_up"),
    "manu_prod_spd[001]": ("莱茵科技·α", "tier_0"),
    "manu_prod_spd[011]": ("莱茵科技·β", "tier_0"),
    "manu_prod_spd[002]": ("红松骑士团·α", "tier_0"),
    "manu_prod_spd[012]": ("红松骑士团·β", "tier_0"),
    "manu_prod_spd[000]": ("标准化·α", "tier_0"),
    "manu_prod_spd[010]": ("标准化·β", "tier_0"),
    "manu_prod_spd[020]": ("咪波·制造型", "tier_up"),
    "manu_prod_spd[021]": ("莱茵科技·γ", "tier_up"),
    "manu_formula_spd[010]": ("作战指导录像", "tier_0"),
    "manu_formula_spd[020]": ("拳术指导录像", "tier_0"),
    "manu_formula_spd[022]": ("逆境荣光", "tier_up"),
    "manu_formula_spd[100]": ("金属工艺·α", "tier_0"),
    "manu_formula_spd[101]": ("金属工艺·α", "tier_0"),
    "manu_formula_spd[110]": ("金属工艺·β", "tier_up"),
    "manu_formula_spd[200]": ("源石工艺·α", "tier_0"),
    "manu_formula_spd[201]": ("地质学·α", "tier_0"),
    "manu_formula_spd[210]": ("源石工艺·β", "tier_up"),
    "manu_formula_spd[211]": ("地质学·β", "tier_up"),
    "manu_formula_spd[212]": ("源石研究", "tier_up"),
    "manu_formula_spd[213]": ("火山学家", "tier_up"),
    "manu_prod_spd&limit[000]": ("仓库整备·α", "tier_0"),
    "manu_prod_spd&limit[001]": ("仓库整备·β", "tier_up"),
    "manu_prod_limit&cost[0000]": ("拾荒者", "tier_0"),
    "manu_prod_limit&cost[000]": ("拾荒者", "tier_0"),
    "manu_prod_limit&cost[010]": ("囤积者", "tier_0"),
    "manu_prod_limit&cost[011]": ("无畏豪情", "tier_0"),
    "manu_prod_limit&cost[020]": ("探险者", "tier_0"),
    "manu_prod_limit&cost[021]": ("掘进工程", "tier_0"),
    "manu_prod_limit&cost[1020]": ("收纳达人", "tier_0"),
    "manu_prod_spd&limit&cost[000]": ("工匠精神·α", "tier_0"),
    "manu_prod_spd&limit&cost[001]": ("工匠精神·β", "tier_up"),
    "manu_prod_spd&limit&cost[010]": ("麻烦制造者", "tier_0"),
    "manu_prod_spd&limit&cost[011]": ("特立独行", "tier_up"),
    "manu_prod_spd&limit&cost[020]": ("“可靠”助手", "tier_0"),
    "manu_prod_spd&limit&cost[300]": ("行动派", "tier_0"),
    "manu_formula_limit[0000]": ("剪辑·α", "tier_up"),
    "manu_formula_limit[010]": ("剪辑·β", "tier_up"),
    "manu_formula_cost[000]": ("Vlog", "tier_0"),
    "manu_cost[000]": ("春雷响，万物长", "tier_0"),
    "manu_prod_spd&trade[000]": ("再生能源", "tier_0"),
    "manu_prod_spd_bd[000]": ("念力", "tier_0"),
    "manu_prod_spd_bd[010]": ("意识实体", "tier_up"),
    "manu_prod_spd_bd[200]": ("逐水草", "tier_0"),
    "manu_prod_spd_bd[201]": ("问枯荣", "tier_up"),
    "manu_prod_spd_bd[300]": ("稻禾厚，顺秋收", "tier_up"),
    "manu_prod_spd_bd_n1[000]": ("超感", "tier_0"),
    "manu_prod_spd_reduce[000]": ("模糊视线", "tier_0"),
    "manu_prod_bd[000]": ("实干的寡言者", "tier_up"),
}


@dataclass
class ParsedSkill:
    skill_name: str
    tier: str
    description: str
    atoms: list[dict] = field(default_factory=list)


def load_prts() -> dict[str, tuple[str, str]]:
    rows = json.loads((DATA / "prts_manufacturing_skills.json").read_text(encoding="utf-8"))[
        "rows"
    ]
    return {r["skill_name"]: (r["description"], r["operators"]) for r in rows}


def load_instances() -> tuple[dict[str, list[str]], dict[str, dict]]:
    data = json.loads((DATA / "operator_instances.json").read_text(encoding="utf-8"))
    buff_ops: dict[str, list[str]] = {}
    op_buffs: dict[str, dict] = {}
    for key, inst in data["instances"].items():
        manu = inst.get("facilities", {}).get("manufacture")
        if not manu:
            continue
        name = inst["name"]
        op_buffs.setdefault(name, {"tier_0": [], "tier_up": []})
        tier = inst["tier"]
        for bid in manu["buff_ids"]:
            buff_ops.setdefault(bid, [])
            if name not in buff_ops[bid]:
                buff_ops[bid].append(name)
            op_buffs[name][tier].extend(manu["buff_ids"])
    return buff_ops, op_buffs


def all_manu_buff_ids(buff_ops: dict[str, list[str]]) -> list[str]:
    return sorted(buff_ops.keys())


def pick_operator(buff_id: str, buff_ops: dict[str, list[str]]) -> str:
    ops = buff_ops[buff_id]
    return ops[0] if len(ops) == 1 else min(ops, key=len)


def resolve_skill_name(buff_id: str, op: str, op_buffs: dict[str, dict]) -> tuple[str, str]:
    if buff_id in MANUAL_SKILL:
        return MANUAL_SKILL[buff_id]
    t0 = op_buffs.get(op, {}).get("tier_0", [])
    up = op_buffs.get(op, {}).get("tier_up", [])
    in_t0 = buff_id in t0
    in_up = buff_id in up
    tier = "tier_up" if in_up and not in_t0 else "tier_0"
    if in_t0 and in_up:
        # same buff both tiers -> tier_0 label
        tier = "tier_0"
    prts_by_name = load_prts()
    op_skills = [
        name
        for name, (_, ops) in prts_by_name.items()
        if op in re.split(r"[；;]", ops)
    ]
    if len(op_skills) == 1:
        return op_skills[0], tier
    # prefer α/β in name vs tier
    if tier == "tier_0":
        for s in op_skills:
            if "·α" in s or s.endswith("α"):
                return s, tier
    else:
        for s in op_skills:
            if "·β" in s or s.endswith("β") or "精" in s:
                return s, tier
    return op_skills[0], tier


def parse_atoms(description: str, buff_id: str) -> list[dict]:
    atoms: list[dict] = []
    order = 20

    def add_prod(value: float, recipe: str | None = None) -> None:
        nonlocal order
        action: dict = {
            "kind": "add_flat_eff",
            "params": {"value": value},
        }
        if recipe:
            action["params"]["recipe"] = recipe
        atoms.append({"action": action, "phase": "constant", "phase_order": order})
        order += 1

    def add_storage(delta: int, recipe: str | None = None) -> None:
        nonlocal order
        action: dict = {
            "kind": "add_limit_delta",
            "params": {"delta": delta},
        }
        if recipe:
            action["params"]["recipe"] = recipe
        atoms.append({"action": action, "phase": "limit", "phase_order": order})
        order += 1

    def add_mood(delta: float, scope: str = "self", recipe: str | None = None) -> None:
        nonlocal order
        atom: dict = {
            "action": {
                "kind": "mood_drain_delta",
                "params": {"delta": delta, "scope": scope},
            },
            "phase": "mood",
            "phase_order": order,
        }
        if recipe:
            atom["condition"] = {"active_recipe": {"kind": recipe}}
        atoms.append(atom)
        order += 1

    # trade station count -> gold formula prod
    m = re.search(
        r"每个贸易站为当前制造站贵金属类配方的生产力\+(\d+(?:\.\d+)?)%", description
    )
    if m:
        rate = float(m.group(1))
        atoms.append(
            {
                "selector": "trade_station_count",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": rate, "recipe": "gold"},
                },
                "phase": "constant",
                "phase_order": 20,
            }
        )
        return atoms

    # room-wide mood (黍)
    m = re.search(r"当前制造站内所有干员心情每小时消耗([+-]\d+(?:\.\d+)?)", description)
    if m and "生产力" not in description:
        atoms.append(
            {
                "action": {
                    "kind": "mood_drain_delta",
                    "params": {"delta": float(m.group(1)), "scope": "room_operators"},
                },
                "phase": "mood",
                "phase_order": 10,
            }
        )
        return atoms

    # recipe-specific storage
    for pat, recipe in (
        (r"生产作战记录类配方时，仓库容量上限\+(\d+)", "battle_record"),
        (r"生产作战记录类配方时，仓库容量\+(\d+)", "battle_record"),
    ):
        m = re.search(pat, description)
        if m:
            add_storage(int(m.group(1)), recipe)
            break

    # recipe-specific mood only
    if re.search(r"生产作战记录类配方时，心情每小时消耗", description):
        m = re.search(r"心情每小时消耗([+-]\d+(?:\.\d+)?)", description)
        if m and not re.search(r"类配方的生产力", description):
            add_mood(float(m.group(1)), recipe="battle_record")
            if atoms:
                return atoms

    # formula productivity
    for pat, recipe in (
        (r"作战记录类配方的生产力\+(\d+(?:\.\d+)?)%", "battle_record"),
        (r"贵金属类配方的生产力\+(\d+(?:\.\d+)?)%", "gold"),
        (r"源石类配方的生产力\+(\d+(?:\.\d+)?)%", "originium"),
    ):
        m = re.search(pat, description)
        if m:
            add_prod(float(m.group(1)), recipe)

    # all-recipe productivity (+ or -)
    if not any(a.get("action", {}).get("params", {}).get("recipe") for a in atoms):
        m = re.search(r"生产力\+(\d+(?:\.\d+)?)%", description)
        if m:
            add_prod(float(m.group(1)))
        m = re.search(r"生产力-(\d+(?:\.\d+)?)%", description)
        if m:
            add_prod(-float(m.group(1)))

    # storage (general)
    if not any(a.get("action", {}).get("kind") == "add_limit_delta" for a in atoms):
        m = re.search(r"仓库容量上限\+(\d+)", description)
        if m:
            add_storage(int(m.group(1)))
        m = re.search(r"仓库容量上限-(\d+)", description)
        if m:
            add_storage(-int(m.group(1)))
        m = re.search(r"仓库容量\+(\d+)", description)
        if m and "生产力" in description and "每格" not in description:
            add_storage(int(m.group(1)))

    # mood (self)
    if not any(a.get("phase") == "mood" for a in atoms):
        m = re.search(r"心情每小时消耗([+-]\d+(?:\.\d+)?)", description)
        if m:
            add_mood(float(m.group(1)))

    # 木天蓼 flat part of 泰拉调查团
    if buff_id == "manu_prod_spd&limit&bd[000]":
        atoms = [
            {
                "action": {"kind": "add_limit_delta", "params": {"delta": 8}},
                "phase": "limit",
                "phase_order": 20,
            },
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 21,
            },
        ]

    return atoms


def build_skill_def(buff_id: str, buff_ops: dict[str, list[str]], op_buffs: dict[str, dict]) -> dict:
    prts = load_prts()
    if buff_id in L2_DELEGATE:
        op = pick_operator(buff_id, buff_ops)
        skill_name, tier = resolve_skill_name(buff_id, op, op_buffs)
        return {
            "id": buff_id,
            "skill_name": skill_name,
            "facility": "manufacture",
            "tier": tier,
            "atoms": [],
        }

    op = pick_operator(buff_id, buff_ops)
    skill_name, tier = resolve_skill_name(buff_id, op, op_buffs)
    desc = prts.get(skill_name, ("", ""))[0]
    if not desc:
        raise RuntimeError(f"no PRTS description for {buff_id} -> {skill_name} ({op})")
    atoms = parse_atoms(desc, buff_id)
    if not atoms:
        raise RuntimeError(f"failed to parse atoms for {buff_id}: {desc}")
    return {
        "id": buff_id,
        "skill_name": skill_name,
        "facility": "manufacture",
        "tier": tier,
        "atoms": atoms,
    }


def main() -> None:
    buff_ops, op_buffs = load_instances()
    buff_ids = all_manu_buff_ids(buff_ops)
    existing = json.loads((DATA / "skill_table.json").read_text(encoding="utf-8"))
    existing_ids = {s["id"] for s in existing["skills"]}
    kept = list(existing["skills"])

    new_defs: list[dict] = []
    errors: list[str] = []
    for bid in buff_ids:
        if bid in existing_ids:
            continue
        try:
            new_defs.append(build_skill_def(bid, buff_ops, op_buffs))
        except RuntimeError as exc:
            errors.append(f"{bid}: {exc}")

    # keep trade skills first, append manufacture sorted by id
    manufacture = sorted(new_defs, key=lambda s: s["id"])
    out = {"version": existing.get("version", 1), "skills": kept + manufacture}
    (DATA / "skill_table.json").write_text(
        json.dumps(out, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    const_n = sum(1 for s in manufacture if s["atoms"])
    deleg_n = sum(1 for s in manufacture if not s["atoms"])
    print(f"added {len(manufacture)} manufacture skills ({const_n} constant, {deleg_n} L2 delegate)")
    if errors:
        print("ERRORS:")
        for e in errors:
            print(f"  {e}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
