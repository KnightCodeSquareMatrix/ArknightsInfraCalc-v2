#!/usr/bin/env python3
"""Append power-station skill entries to data/skill_table.json."""

from __future__ import annotations

import csv
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"

# L2 / cross-facility: registered with empty atoms.
L2_DELEGATE = {
    "power_rec_spd&addition[000]",
    "power_rec_spd&addition[001]",
    "power_prod_spd_P[000]",
}

# buff_id -> (skill_name, tier)
BUFF_META: dict[str, tuple[str, str]] = {
    "power_rec_spd[000]": ("备用能源", "tier_0"),
    "power_rec_spd[001]": ("热能充能·α", "tier_0"),
    "power_rec_spd[002]": ("光能充能·α", "tier_0"),
    "power_rec_spd[003]": ("电磁充能·α", "tier_0"),
    "power_rec_spd[004]": ("挽歌充能·α", "tier_0"),
    "power_rec_spd[005]": ("灵河充能·α", "tier_0"),
    "power_rec_spd[006]": ("充能中", "tier_0"),
    "power_rec_spd[007]": ("雷法专精", "tier_0"),
    "power_rec_spd[008]": ("澎湃紊流", "tier_0"),
    "power_rec_spd[009]": ("澎湃紊流", "tier_up"),
    "power_rec_spd[010]": ("设备维护", "tier_up"),
    "power_rec_spd[011]": ("清洁能源", "tier_0"),
    "power_rec_spd[013]": ("高热充能", "tier_up"),
    "power_rec_spd[014]": ("脉冲电弧·α", "tier_0"),
    "power_rec_spd[015]": ("电磁充能·β", "tier_up"),
    "power_rec_spd[016]": ("聚能", "tier_up"),
    "power_rec_spd[017]": ("灯塔供能模块", "tier_up"),
    "power_rec_spd[020]": ("静电场", "tier_0"),
    "power_rec_spd[021]": ("电荷释放", "tier_up"),
    "power_rec_spd[022]": ("热能充能·γ", "tier_up"),
    "power_rec_spd[023]": ("电荷释放", "tier_up"),
    "power_rec_spd[024]": ("合法窃电", "tier_up"),
    "power_rec_spd[025]": ("穹顶物流管理·α", "tier_0"),
    "power_rec_spd[026]": ("穹顶物流管理·β", "tier_up"),
    "power_rec_spd[0010]": ("鸡电工程", "tier_0"),
    "power_rec_spd[1022]": ("热能充能·γ", "tier_up"),
    "power_rec_spd&cost[000]": ("热情澎湃", "tier_0"),
    "power_rec_spd&cost[010]": ("外卖水果挞", "tier_0"),
    "power_rec_spd&addition[000]": ("技术交流·α", "tier_0"),
    "power_rec_spd&addition[001]": ("技术交流·β", "tier_up"),
    "power_rec_spd_P[000]": ("“愉快的对谈”", "tier_0"),
    "power_rec_spd_P[001]": ("咒文共鸣", "tier_0"),
    "power_rec_spd_ext&faction[000]": ("维护中", "tier_0"),
    "power_rec_spd_ext&tag[000]": ("鸡励机制", "tier_0"),
    "power_rec_drone[000]": ("巡线框架", "tier_0"),
    "power_count[000]": ("晨曦", "tier_up"),
    "power_rec_rhine[000]": ("生态科主任", "tier_up"),
    "power_prod_spd_P[000]": ("“滴滴，启动！”", "tier_0"),
    "power_rec_spd&dorm&lv[000]": ("灵河共鸣", "tier_up"),
}

# Numeric suffix of power_rec_spd[NNN] -> flat charge speed %.
REC_SPD_PCT: dict[str, float] = {
    "000": 10,
    "001": 10,
    "002": 10,
    "003": 10,
    "004": 10,
    "005": 10,
    "006": 10,
    "007": 10,
    "008": 10,
    "009": 15,
    "010": 15,
    "011": 15,
    "013": 15,
    "014": 15,
    "015": 15,
    "016": 15,
    "017": 15,
    "020": 20,
    "021": 20,
    "022": 10,
    "023": 20,
    "024": 20,
    "025": 15,
    "026": 20,
    "0010": 10,
    "1022": 20,
}


def flat_charge(value: float, order: int = 20) -> dict:
    return {
        "action": {"kind": "add_flat_eff", "params": {"value": value}},
        "phase": "constant",
        "phase_order": order,
    }


def mood_atom(delta: float) -> dict:
    return {
        "action": {
            "kind": "mood_drain_delta",
            "params": {"delta": delta, "scope": "self"},
        },
        "phase": "mood",
        "phase_order": 10,
    }


def atoms_for(buff_id: str) -> list[dict]:
    if buff_id in L2_DELEGATE:
        return []

    if buff_id.startswith("power_rec_spd[") and "&" not in buff_id:
        key = buff_id.split("[", 1)[1].rstrip("]")
        pct = REC_SPD_PCT.get(key)
        if pct is None:
            raise RuntimeError(f"unknown power_rec_spd suffix {buff_id}")
        return [flat_charge(pct)]

    if buff_id == "power_rec_spd&cost[000]":
        return [mood_atom(-0.52)]

    if buff_id == "power_rec_spd&cost[010]":
        return [mood_atom(-0.3)]

    if buff_id == "power_rec_drone[000]":
        return [
            {
                "selector": "drone_cap",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 0.1, "cap": 25.0},
                },
                "phase": "eff_var",
                "phase_order": 20,
            }
        ]

    if buff_id == "power_rec_spd&dorm&lv[000]":
        return [
            {
                "selector": "dorm_level_sum",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 0.5},
                },
                "phase": "eff_var",
                "phase_order": 20,
            }
        ]

    if buff_id == "power_rec_rhine[000]":
        return [
            flat_charge(10.0, 20),
            {
                "selector": "rhine_life_in_base",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 3.0, "cap": 15.0},
                },
                "phase": "eff_var",
                "phase_order": 21,
            },
        ]

    if buff_id == "power_count[000]":
        return [
            {
                "condition": {"no_platform_in_other_power": {}},
                "action": {
                    "kind": "state_produce",
                    "params": {"key": "VirtualPower", "amount": 1.0},
                },
                "phase": "state_write",
                "phase_order": 10,
            }
        ]

    if buff_id == "power_rec_spd_P[000]":
        return [
            {
                "condition": {"operator_in_base": {"name": "凯尔希"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 21,
            }
        ]

    if buff_id == "power_rec_spd_P[001]":
        return [
            {
                "condition": {"operator_in_training": {"name": "逻各斯"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 21,
            }
        ]

    if buff_id == "power_rec_spd_ext&faction[000]":
        return [
            {
                "condition": {"other_laterano_in_power": {}},
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 21,
            }
        ]

    if buff_id == "power_rec_spd_ext&tag[000]":
        return [
            {
                "condition": {"other_platform_in_power": {}},
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 21,
            }
        ]

    raise RuntimeError(f"no atom template for {buff_id}")


def load_power_buff_ids() -> set[str]:
    inst = json.loads((DATA / "operator_instances.json").read_text(encoding="utf-8"))
    out: set[str] = set()
    for v in inst["instances"].values():
        p = v.get("facilities", {}).get("power")
        if p:
            out.update(p["buff_ids"])
    return out


def build_skill_def(buff_id: str) -> dict:
    if buff_id not in BUFF_META:
        raise RuntimeError(f"missing BUFF_META for {buff_id}")
    skill_name, tier = BUFF_META[buff_id]
    atoms = atoms_for(buff_id)
    return {
        "id": buff_id,
        "skill_name": skill_name,
        "facility": "power",
        "tier": tier,
        "atoms": atoms,
    }


def main() -> None:
    buff_ids = sorted(load_power_buff_ids())
    existing = json.loads((DATA / "skill_table.json").read_text(encoding="utf-8"))
    existing_ids = {s["id"] for s in existing["skills"]}
    kept = [s for s in existing["skills"] if not s["id"].startswith("power_")]

    new_defs: list[dict] = []
    errors: list[str] = []
    for bid in buff_ids:
        if bid in existing_ids and bid.startswith("power_"):
            continue
        try:
            new_defs.append(build_skill_def(bid))
        except RuntimeError as exc:
            errors.append(f"{bid}: {exc}")

    missing_meta = set(buff_ids) - set(BUFF_META)
    for bid in sorted(missing_meta):
        errors.append(f"{bid}: missing BUFF_META")

    out = {"version": existing.get("version", 1), "skills": kept + sorted(new_defs, key=lambda s: s["id"])}
    (DATA / "skill_table.json").write_text(
        json.dumps(out, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"added {len(new_defs)} power skills")
    if errors:
        print("ERRORS:")
        for e in errors:
            print(f"  {e}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
