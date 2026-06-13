#!/usr/bin/env python3
"""Append remaining trade skill_table entries (plain json merge, no regex)."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TABLE = ROOT / "data" / "skill_table.json"

NEW_SKILLS = [
    {
        "id": "trade_ord_spd&multiPar[100]",
        "skill_name": "相伴",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 20.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "condition": {"partner_in_room": {"name": "能天使"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 25.0}},
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_spd&par[001]",
        "skill_name": "外贸决议·β",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 30.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "condition": {"tag_present_in_room": {"tag": "cc.g.glasgow"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 10.0}},
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_spd_par[000]",
        "skill_name": "帮派指南针",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "selector": {"tagged_count_in_room": {"tag": "cc.g.glasgow"}},
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 20.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "condition": {"partner_in_room": {"name": "推进之王"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 35.0}},
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_spd_par[001]",
        "skill_name": "同城加急单",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "selector": {"tagged_count_in_room": {"tag": "cc.g.laterano"}},
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 15.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_par&per[000]",
        "skill_name": "白手起家·α",
        "facility": "trade",
        "tier": "tier_0",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 25.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "condition": {"operator_in_base": {"name": "伊内丝"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_par&per[001]",
        "skill_name": "白手起家·β",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 30.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "condition": {"operator_in_base": {"name": "伊内丝"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 21,
            },
            {
                "condition": {"operator_in_base": {"name": "W"}},
                "action": {"kind": "add_flat_eff", "params": {"value": 5.0}},
                "phase": "constant",
                "phase_order": 22,
            },
        ],
    },
    {
        "id": "trade_ord_pepe[000]",
        "skill_name": "慧眼独到",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {
                    "kind": "peer_eff_absorb",
                    "params": {"rate_per_peer": 0.0},
                },
                "phase": "peer_absorb",
                "phase_order": 10,
            },
            {
                "action": {
                    "kind": "tag_order",
                    "params": {"tag": "pepe_exclusive"},
                },
                "phase": "order_mechanic",
                "phase_order": 5,
            },
        ],
    },
    {
        "id": "trade_ord_spd&dorm&lv[000]",
        "skill_name": "虔诚筹款·α",
        "facility": "trade",
        "tier": "tier_0",
        "atoms": [
            {
                "selector": "dorm_level_sum",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 1.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_spd&dorm&lv[010]",
        "skill_name": "虔诚筹款·β",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "selector": "dorm_level_sum",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 2.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_spd&formula[000]",
        "skill_name": "精准排期",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 30.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "selector": "manu_recipe_kinds",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 2.0},
                },
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_spd&meet[000]",
        "skill_name": "新城贸易",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 25.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "selector": "meeting_max_level",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 5.0, "cap": 40.0},
                },
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_spd&meet[010]",
        "skill_name": "天生的顾问",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 15.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "selector": "meeting_max_level",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 5.0, "cap": 30.0},
                },
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_spd&tag[010]",
        "skill_name": "精英小队",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 25.0}},
                "phase": "constant",
                "phase_order": 20,
            },
            {
                "selector": "elite_facility_count",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 2.0, "cap": 20.0},
                },
                "phase": "constant",
                "phase_order": 21,
            },
        ],
    },
    {
        "id": "trade_ord_spd&tag[020]",
        "skill_name": "“孺子可教！”",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "selector": "sui_facility_count",
                "action": {
                    "kind": "add_flat_eff_from_selector",
                    "params": {"multiplier": 4.0, "cap": 20.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_spd&wt[000]",
        "skill_name": "天真的谈判者",
        "facility": "trade",
        "tier": "tier_0",
        "atoms": [
            {
                "action": {"kind": "add_flat_eff", "params": {"value": 10.0}},
                "phase": "order_mechanic",
                "phase_order": 5,
            },
            {
                "action": {
                    "kind": "tag_order",
                    "params": {"tag": "eureka"},
                },
                "phase": "order_mechanic",
                "phase_order": 6,
            },
        ],
    },
    {
        "id": "trade_ord_spd_bd_n1[000]",
        "skill_name": "乐感",
        "facility": "trade",
        "tier": "tier_0",
        "atoms": [
            {
                "selector": "dorm_occupant_count",
                "action": {
                    "kind": "state_produce",
                    "params": {"key": "Perception", "amount": 1.0},
                },
                "phase": "state_write",
                "phase_order": 10,
            },
            {
                "action": {
                    "kind": "state_convert",
                    "params": {
                        "from": "Perception",
                        "to": "SilentEcho",
                        "ratio": 1.0,
                    },
                },
                "phase": "state_write",
                "phase_order": 11,
            },
        ],
    },
    {
        "id": "trade_ord_spd_bd[000]",
        "skill_name": "徘徊旋律",
        "facility": "trade",
        "tier": "tier_0",
        "atoms": [
            {
                "action": {
                    "kind": "state_consume_to_eff",
                    "params": {"key": "SilentEcho", "div": 4.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_spd_bd[010]",
        "skill_name": "怅惘和声",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {
                    "kind": "state_consume_to_eff",
                    "params": {"key": "SilentEcho", "div": 2.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_spd_bd[100]",
        "skill_name": "熟悉的味道",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {
                    "kind": "state_consume_to_eff",
                    "params": {"key": "MonsterCuisine", "div": 1.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_spd_bd_n2[000]",
        "skill_name": "“愿者上钩”",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "selector": "dorm_occupant_count",
                "action": {
                    "kind": "state_produce",
                    "params": {"key": "HumanFireworks", "amount": 1.0},
                },
                "phase": "state_write",
                "phase_order": 10,
            },
            {
                "action": {
                    "kind": "state_consume_to_eff",
                    "params": {"key": "HumanFireworks", "div": 1.0},
                },
                "phase": "constant",
                "phase_order": 20,
            },
        ],
    },
    {
        "id": "trade_ord_spd_variable2[000]",
        "skill_name": "天道酬勤·α",
        "facility": "trade",
        "tier": "tier_0",
        "atoms": [
            {
                "selector": "other_ops_total_eff",
                "action": {
                    "kind": "add_bucket_eff_from_selector",
                    "params": {"step": 5.0, "ret_per_step": 5.0, "cap": 25.0},
                },
                "phase": "eff_var",
                "phase_order": 10,
            },
        ],
    },
    {
        "id": "trade_ord_spd_variable2[001]",
        "skill_name": "天道酬勤·β",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "selector": "other_ops_total_eff",
                "action": {
                    "kind": "add_bucket_eff_from_selector",
                    "params": {"step": 5.0, "ret_per_step": 5.0, "cap": 35.0},
                },
                "phase": "eff_var",
                "phase_order": 10,
            },
        ],
    },
    {
        "id": "trade_ord_spd_variable3[000]",
        "skill_name": "冠军风采",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "selector": "limit_contrib_sum",
                "action": {
                    "kind": "add_bucket_eff_from_selector",
                    "params": {"step": 5.0, "ret_per_step": 25.0, "cap": 100.0},
                },
                "phase": "limit_var",
                "phase_order": 10,
            },
        ],
    },
    {
        "id": "trade_ord_wt&cost[012]",
        "skill_name": "鉴定师的手段",
        "facility": "trade",
        "tier": "tier_up",
        "atoms": [
            {
                "action": {
                    "kind": "tag_order",
                    "params": {"tag": "tailor_beta"},
                },
                "phase": "order_mechanic",
                "phase_order": 10,
            },
            {
                "action": {
                    "kind": "mood_drain_delta",
                    "params": {"delta": -0.25, "scope": "self"},
                },
                "phase": "mood",
                "phase_order": 10,
            },
        ],
    },
]

MOOD_P010 = [
    {
        "condition": {"partner_in_room": {"name": "能天使"}},
        "action": {
            "kind": "mood_drain_delta",
            "params": {"delta": -0.3, "scope": "self"},
        },
        "phase": "mood",
        "phase_order": 10,
    },
]


def main() -> None:
    data = json.loads(TABLE.read_text(encoding="utf-8"))
    by_id = {s["id"]: s for s in data["skills"]}
    for skill in NEW_SKILLS:
        if skill["id"] in by_id:
            print("skip existing", skill["id"])
            continue
        data["skills"].append(skill)
        print("added", skill["id"])
    if "trade_ord_limit&cost_P[010]" in by_id:
        by_id["trade_ord_limit&cost_P[010]"]["atoms"] = MOOD_P010
        print("updated trade_ord_limit&cost_P[010]")
    TABLE.write_text(
        json.dumps(data, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"total skills: {len(data['skills'])}")


if __name__ == "__main__":
    main()
