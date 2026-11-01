#!/usr/bin/env python3
"""Generate the bounded office/reception registry from game data.

The registry retains MECHANICS_REGISTRY rows 172-214 and 324-390, plus explicit supplemental skills.
Mechanics remain explicit below; game data only supplies buff ids and unlocks.
"""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path


OFFICE_ROWS = range(172, 215)
MEETING_ROWS = range(324, 391)

IGNORED = {
    172: "unknown clue probability",
    173: "unknown clue probability",
    174: "unknown clue probability",
    324: "unknown clue probability",
    325: "unknown clue probability",
    326: "unknown clue probability",
    327: "unknown clue probability",
    328: "unknown clue probability",
    330: "unknown missing-clue probability",
    333: "next-clue event history",
    334: "next-clue event history",
    348: "unknown clue probability",
    349: "unknown clue probability",
    350: "unknown clue probability",
    351: "unknown clue probability",
    352: "unknown clue probability",
    353: "unknown clue probability",
    354: "unknown clue probability",
    355: "unknown clue probability",
    356: "unknown clue probability",
    357: "unknown clue probability",
    373: "unknown owned-clue probability",
    380: "unknown missing-clue probability",
    381: "unknown missing-clue probability",
    382: "unknown missing-clue probability",
    383: "unknown missing-clue probability",
    384: "unknown missing-clue probability",
    385: "unknown missing-clue probability",
    386: "unknown faction-clue probability",
    387: "unknown clue probability",
    388: "unknown clue probability",
    389: "unknown clue probability",
    390: "unknown clue probability",
}

UNSUPPORTED = {
    190: "mood-exhaustion resource lifecycle",
    191: "memory-fragment production remains in the global-resource owner",
    192: "human-fireworks production remains in the global-resource owner",
    193: "silent-echo production is not represented by the current global owner",
    196: "operator-in-control-center condition",
    344: "peer faction condition",
    345: "peer faction condition",
    346: "peer faction condition",
    378: "operator-in-dormitory condition",
}

OFFICE_MEETING_PER_SLOT = {176, 177, 178, 179, 180}
SOLO_SPEED = {337: 50, 338: 15, 339: 35, 340: 15, 341: 35, 377: 35}
PARTNER_SPEED = {329: ("提丰", 15), 330: ("提丰", 15), 331: ("铃兰", 30)}
MOOD_DELTA = {
    195: -0.25,
    196: -0.25,
    201: -0.25,
    202: -0.25,
    203: -0.25,
    204: -0.25,
    205: -0.25,
    206: -0.25,
    207: -0.25,
    208: 2.0,
    209: 0.5,
    210: 1.0,
    211: 1.0,
    329: 0.5,
    330: 0.5,
    336: 2.0,
    337: 2.0,
    338: 1.0,
    339: 1.0,
    340: 1.0,
    341: 1.0,
    344: 0.5,
    345: 0.5,
}


def effects_for(row_id: int, efficiency: str) -> list[dict]:
    if row_id in {342, 343}:
        return [{"kind": "speed_flat", "value": 30.0}]
    effects: list[dict] = []
    if efficiency and row_id not in SOLO_SPEED:
        effects.append({"kind": "speed_flat", "value": float(efficiency)})

    if row_id in OFFICE_MEETING_PER_SLOT:
        effects.append({"kind": "meeting_speed_per_extra_recruit_slot", "value": 5.0})
    if row_id == 197:
        effects.append({"kind": "mood_per_extra_recruit_slot", "value": -0.1})
    if row_id == 199:
        effects.append({"kind": "speed_per_extra_recruit_slot", "value": 10.0})
    if row_id == 212:
        effects.append({"kind": "speed_per_dorm_level", "value": 1.0})
    if row_id == 213:
        effects.append({"kind": "speed_per_dorm_level", "value": 2.0})
    if row_id == 214:
        effects.append({"kind": "speed_per_elite_facility", "value": 4.0, "cap": 5})
    if row_id == 194:
        effects.extend(
            [
                {"kind": "speed_per_state", "state": "IntelligenceReserve", "step": 1.0, "value": 5.0},
                {"kind": "speed_per_state", "state": "UsautDrink", "step": 1.0, "value": 5.0},
            ]
        )
    if row_id == 332:
        effects.append({"kind": "speed_per_extra_recruit_slot", "value": 5.0})
    if row_id == 374:
        effects.append({"kind": "speed_per_state", "state": "IntelligenceReserve", "step": 1.0, "value": 5.0})
    if row_id == 375:
        effects.append({"kind": "speed_per_state", "state": "MonsterCuisine", "step": 1.0, "value": 2.0})
    if row_id == 376:
        effects.append({"kind": "speed_per_state", "state": "HumanFireworks", "step": 10.0, "value": 1.0})
    if row_id == 379:
        effects = [{"kind": "speed_ramp_average", "initial": 20.0, "per_hour": 2.0, "cap": 30.0}]
    if row_id in SOLO_SPEED:
        effects.append({"kind": "speed_if_solo", "value": float(SOLO_SPEED[row_id])})
    if row_id in PARTNER_SPEED:
        partner, value = PARTNER_SPEED[row_id]
        effects.append({"kind": "speed_if_partner", "operator": partner, "value": float(value)})
    if row_id in MOOD_DELTA:
        effects.append({"kind": "mood_delta", "value": MOOD_DELTA[row_id]})
    return effects


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--building-data", type=Path, required=True)
    parser.add_argument("--character-table", type=Path, required=True)
    parser.add_argument("--registry", type=Path, default=Path("data/MECHANICS_REGISTRY.csv"))
    parser.add_argument("--output", type=Path, default=Path("data/support_skill_registry.json"))
    args = parser.parse_args()

    building = json.loads(args.building_data.read_text(encoding="utf-8"))
    characters = json.loads(args.character_table.read_text(encoding="utf-8"))
    with args.registry.open(encoding="utf-8-sig", newline="") as stream:
        rows = {int(row["序号"]): row for row in csv.DictReader(stream)}

    wanted = set(OFFICE_ROWS) | set(MEETING_ROWS)
    name_to_id = {
        data["name"]: char_id
        for char_id, data in characters.items()
        if isinstance(data, dict) and data.get("name")
    }
    entries = []
    for row_id in sorted(wanted):
        row = rows[row_id]
        room_type = "HIRE" if row["工作设施"] == "办公室" else "MEETING"
        facility = "office" if room_type == "HIRE" else "meeting"
        for name in row["干员"].split("；"):
            char_id = name_to_id.get(name)
            if char_id is None or char_id not in building["chars"]:
                raise SystemExit(f"missing game-data operator for row {row_id}: {name}")
            matched = []
            for slot, slot_data in enumerate(building["chars"][char_id]["buffChar"]):
                for buff_data in slot_data["buffData"]:
                    buff = building["buffs"].get(buff_data["buffId"], {})
                    if buff.get("roomType") == room_type and buff.get("buffName") == row["技能名"]:
                        matched.append((slot, buff_data))
            if not matched:
                raise SystemExit(f"missing game-data buff for row {row_id}: {name} / {row['技能名']}")
            for slot, buff_data in matched:
                cond = buff_data["cond"]
                entries.append(
                    {
                        "row_id": row_id,
                        "facility": facility,
                        "operator": name,
                        "skill": row["技能名"],
                        "buff_id": buff_data["buffId"],
                        "slot": slot,
                        "min_elite": int(str(cond["phase"]).removeprefix("PHASE_")),
                        "min_level": int(cond["level"]),
                        "effects": effects_for(row_id, row["效率值"]),
                        "ignored": IGNORED.get(row_id),
                        "unsupported": UNSUPPORTED.get(row_id),
                    }
                )

    covered = {entry["row_id"] for entry in entries}
    if covered != wanted:
        raise SystemExit(f"coverage mismatch: missing={sorted(wanted - covered)} extra={sorted(covered - wanted)}")

    # Stable supplemental row ids; unlocks and slots come from upstream data.
    supplemental = [
        (391, "埃癸斯", "跨领域交流", [{"kind": "speed_flat", "value": 10.0}], "unknown missing-clue probability"),
        (392, "虎狼丸", "虎狼丸的报恩", [{"kind": "speed_flat", "value": 30.0}], None),
        (393, "克莱门莎", "谈判技巧", [{"kind": "speed_flat", "value": 20.0}, {"kind": "mood_delta", "value": 1.0}], None),
        (394, "克莱门莎", "合作原则", [{"kind": "speed_if_room_occupancy_at_least", "count": 2, "value": 15.0}, {"kind": "mood_if_room_occupancy_at_least", "count": 2, "value": 1.0}], None),
        (395, "德·托莱多", "拉拢听众", [{"kind": "speed_flat", "value": 20.0}, {"kind": "mood_delta", "value": -0.25}], None),
        (396, "德·托莱多", "聚众宣讲", [{"kind": "speed_flat", "value": 30.0}, {"kind": "mood_delta", "value": -0.25}], None),
    ]
    for row_id, name, skill_name, effects, ignored in supplemental:
        matched = []
        for slot, slot_data in enumerate(building["chars"][name_to_id[name]]["buffChar"]):
            for buff_data in slot_data["buffData"]:
                buff = building["buffs"][buff_data["buffId"]]
                if buff["buffName"] == skill_name:
                    cond = buff_data["cond"]
                    matched.append({
                        "row_id": row_id, "facility": "office" if buff["roomType"] == "HIRE" else "meeting",
                        "operator": name, "skill": skill_name, "buff_id": buff_data["buffId"], "slot": slot,
                        "min_elite": int(str(cond["phase"]).removeprefix("PHASE_")), "min_level": int(cond["level"]),
                        "effects": effects, "ignored": ignored, "unsupported": None,
                    })
        if len(matched) != 1:
            raise SystemExit(f"ambiguous supplemental game-data skill: {name} / {skill_name}")
        entries.extend(matched)

    output = {
        "version": 1,
        "scope": {"office_rows": [172, 214], "meeting_rows": [324, 390], "supplemental_rows": [391, 396]},
        "entries": entries,
    }
    args.output.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
