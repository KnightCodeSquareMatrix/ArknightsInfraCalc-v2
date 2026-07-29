#!/usr/bin/env python3
"""从公孙 243 排班导出 JSON 生成最高合法练度 operbox + 各班 BaseAssignment。"""

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


def max_legal_progress(rarity: int) -> tuple[int, int]:
    return {
        1: (0, 30),
        2: (0, 30),
        3: (1, 55),
        4: (2, 70),
        5: (2, 80),
        6: (2, 90),
    }[rarity]


def ideal_operbox(schedule_names: set[str], seed_entries: list[dict]) -> list[dict]:
    by_name = index_operbox_by_name(seed_entries)
    out: list[dict] = []
    seen: set[str] = set()

    def add(name: str) -> None:
        if name in seen:
            return
        seen.add(name)
        base = by_name.get(name)
        if base is None:
            raise ValueError(f"missing operbox metadata for {name}")
        entry = dict(base)
        rarity = entry.get("rarity")
        if rarity not in range(1, 7):
            raise ValueError(f"invalid rarity for {name}: {rarity!r}")
        entry["own"] = True
        entry["potential"] = 6
        entry["elite"], entry["level"] = max_legal_progress(rarity)
        out.append(entry)

    for e in seed_entries:
        add(e["name"])

    for name in sorted(schedule_names):
        add(name)

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
        default=root / "data" / "fixtures" / "243" / "schedule_export.json",
    )
    parser.add_argument(
        "--seed-operbox",
        type=Path,
        default=root / "data" / "operbox_gongsun.json",
        help="提供完整 id/稀有度目录；缺干员或非法星级时直接报错",
    )
    parser.add_argument(
        "-o",
        "--out-dir",
        type=Path,
        default=root / "data" / "schedule_243",
    )
    parser.add_argument("--operbox-name", default="operbox_ideal_e2.json")
    parser.add_argument("--operbox-only", action="store_true")
    parser.add_argument(
        "--sync-operbox-mirrors",
        action="store_true",
        help="同步写入 data 与 release 的三份标准 operbox",
    )
    args = parser.parse_args()

    schedule = load_json(args.schedule)
    seed = json.loads(args.seed_operbox.read_text(encoding="utf-8-sig"))
    names = collect_schedule_names(schedule)
    operbox = ideal_operbox(names, seed)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    operbox_paths = [args.out_dir / args.operbox_name]
    if args.sync_operbox_mirrors:
        operbox_paths = [
            root / "data" / "fixtures" / "243" / "operbox_full_e2.json",
            root / "data" / "schedule_243" / "operbox_ideal_e2.json",
            root / "release" / "fixtures" / "operbox_full_e2.json",
        ]
    for operbox_path in operbox_paths:
        operbox_path.parent.mkdir(parents=True, exist_ok=True)
        operbox_path.write_text(
            json.dumps(operbox, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
            newline="\r\n",
        )

    if not args.operbox_only:
        for i, plan in enumerate(schedule.get("plans", [])):
            plan_name = plan.get("name", f"shift_{i}")
            safe = plan_name.replace(" ", "_").replace("/", "_")
            assign_path = args.out_dir / f"assignment_{i}_{safe}.json"
            assign_path.write_text(
                json.dumps(plan_to_assignment(plan), ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )

    print(f"schedule_ops={len(names)} operbox={len(operbox)} -> {args.out_dir}")
    for operbox_path in operbox_paths:
        print(f"  {operbox_path}")
    if not args.operbox_only:
        for i, plan in enumerate(schedule.get("plans", [])):
            print(f"  assignment_{i}_{plan.get('name', i)}")


if __name__ == "__main__":
    main()
