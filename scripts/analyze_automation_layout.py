#!/usr/bin/env python3
"""Report factory occupancy and selected automation facts from Rust rotation JSON."""

import argparse
import json
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--layout", type=Path, required=True)
    parser.add_argument("--rotation-out", type=Path, required=True)
    args = parser.parse_args()

    layout = json.loads(args.layout.read_text(encoding="utf-8"))
    rotation = json.loads(args.rotation_out.read_text(encoding="utf-8"))
    factories = [room for room in layout["rooms"] if room["kind"] == "factory"]
    power_rooms = {room["id"] for room in layout["rooms"] if room["kind"] == "power_plant"}
    alternatives = [
        selected["alternative_id"]
        for selected in rotation["peak_plan"]["selected_rules"]
        if selected["rule_id"] == "automation_group"
    ]
    print(
        f"layout={layout.get('template', args.layout.name)} "
        f"factories={len(factories)} automation_alternative={alternatives or 'none'}"
    )
    for index, shift in enumerate(rotation["shifts"], start=1):
        assignments = {
            room["room_id"]: room["operators"]
            for room in shift["assignment"]["rooms"]
        }
        print(f"shift={index}")
        for room in factories:
            operators = assignments.get(room["id"], [])
            members = ", ".join(
                f"{operator['name']} E{operator['elite']}" for operator in operators
            )
            recipe = room.get("product", {}).get("factory", {}).get("recipe", "unset")
            print(
                f"  {room['id']} level={room['level']} recipe={recipe} "
                f"occupants={len(operators)} [{members}]"
            )
        greyy_rooms = [
            room_id
            for room_id in power_rooms
            if any(
                operator["name"] == "承曦格雷伊"
                for operator in assignments.get(room_id, [])
            )
        ]
        print(f"  greyy_power_rooms={sorted(greyy_rooms)}")


if __name__ == "__main__":
    main()
