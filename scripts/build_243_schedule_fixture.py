#!/usr/bin/env python3
"""从公孙 243 排班导出 JSON 生成全精2 operbox + 各班 BaseAssignment。"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def load_json(path: Path) -> dict:
    text = path.read_text(encoding="utf-8-sig")
    return json.loads(text)


def collect_schedule_names(schedule: dict) -> set[str]:
    names: set[str] = set()
    for plan in schedule.get("plans", []):
        rooms = plan.get("rooms", {})
        for section in ("trading", "manufacture", "power", "dormitory", "control"):
            for slot in rooms.get(section, []):
                for op in slot.get("operators", []):
                    if op:
                        names.add(op)
        for slot in rooms.get("meeting", []):
            for op in slot.get("operators", []):
                if op:
                    names.add(op)
        for slot in rooms.get("hire", []):
            for op in slot.get("operators", []):
                if op:
                    names.add(op)
    return names


def index_operbox_by_name(entries: list[dict]) -> dict[str, dict]:
    by_name: dict[str, dict] = {}
    for e in entries:
        by_name[e["name"]] = dict(e)
    return by_name


def ideal_operbox(
    schedule_names: set[str],
    seed_entries: list[dict],
    elite: int = 2,
    level: int = 90,
) -> list[dict]:
    by_name = index_operbox_by_name(seed_entries)
    out: list[dict] = []
    seen: set[str] = set()

    def add(name: str, fallback_id: str) -> None:
        if name in seen:
            return
        seen.add(name)
        base = by_name.get(name)
        if base:
            entry = dict(base)
        else:
            entry = {
                "id": fallback_id,
                "name": name,
                "elite": 0,
                "level": 1,
                "own": True,
                "potential": 6,
                "rarity": 5,
            }
        entry["own"] = True
        entry["elite"] = elite
        entry["level"] = max(entry.get("level", 0), level)
        out.append(entry)

    for e in seed_entries:
        if e.get("own"):
            add(e["name"], e.get("id", f"seed_{e['name']}"))

    for i, name in enumerate(sorted(schedule_names)):
        add(name, f"schedule_{i:04d}")

    out.sort(key=lambda x: x["name"])
    return out


def plan_to_assignment(plan: dict, elite: int = 2) -> dict:
    rooms = plan.get("rooms", {})
    assignment_rooms: list[dict] = []

    def push(room_id: str, operators: list[str]) -> None:
        ops = [{"name": n, "elite": elite} for n in operators if n]
        if ops:
            assignment_rooms.append({"room_id": room_id, "operators": ops})

    for i, slot in enumerate(rooms.get("trading", []), start=1):
        if slot.get("skip"):
            continue
        push(f"trade_{i}", slot.get("operators", []))

    for i, slot in enumerate(rooms.get("manufacture", []), start=1):
        if slot.get("skip"):
            continue
        push(f"manu_{i}", slot.get("operators", []))

    for i, slot in enumerate(rooms.get("power", []), start=1):
        if slot.get("skip"):
            continue
        push(f"power_{i}", slot.get("operators", []))

    for i, slot in enumerate(rooms.get("dormitory", []), start=1):
        if slot.get("skip"):
            continue
        push(f"dorm_{i}", slot.get("operators", []))

    for slot in rooms.get("control", []):
        if slot.get("skip"):
            continue
        push("control", slot.get("operators", []))

    return {"rooms": assignment_rooms, "base_workforce": []}


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "schedule",
        type=Path,
        nargs="?",
        default=root / "243_layout_3_times_a_day.json",
    )
    parser.add_argument(
        "--seed-operbox",
        type=Path,
        default=root / "data" / "operbox_gongsun.json",
        help="用于 id/稀有度模板；缺干员时补条目",
    )
    parser.add_argument(
        "-o",
        "--out-dir",
        type=Path,
        default=root / "data" / "schedule_243",
    )
    args = parser.parse_args()

    schedule = load_json(args.schedule)
    seed = json.loads(args.seed_operbox.read_text(encoding="utf-8-sig"))
    names = collect_schedule_names(schedule)
    operbox = ideal_operbox(names, seed, elite=2, level=90)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    operbox_path = args.out_dir / "operbox_ideal_e2.json"
    operbox_path.write_text(
        json.dumps(operbox, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    for i, plan in enumerate(schedule.get("plans", [])):
        plan_name = plan.get("name", f"shift_{i}")
        safe = plan_name.replace(" ", "_").replace("/", "_")
        assign_path = args.out_dir / f"assignment_{i}_{safe}.json"
        assign_path.write_text(
            json.dumps(plan_to_assignment(plan), ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )

    print(f"schedule_ops={len(names)} operbox={len(operbox)} -> {args.out_dir}")
    print(f"  {operbox_path.name}")
    for i, plan in enumerate(schedule.get("plans", [])):
        print(f"  assignment_{i}_{plan.get('name', i)}")


if __name__ == "__main__":
    main()
