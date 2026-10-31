#!/usr/bin/env python3
"""Query persisted rotation reports without solving or recomputing mechanics."""

from __future__ import annotations

import argparse
import math
import sys
from pathlib import Path
from typing import Any

if __package__:
    from ._query_output import QueryError, add_output_arguments, emit, pointer, read_json
else:
    from _query_output import QueryError, add_output_arguments, emit, pointer, read_json


def require(obj: dict[str, Any], key: str, kind: type, path: str) -> Any:
    value = obj.get(key)
    if type(value) is not kind:
        raise QueryError(f"{pointer(path, key)} must be {kind.__name__}")
    return value


def number(value: Any, path: str) -> None:
    if type(value) not in (int, float) or not math.isfinite(value):
        raise QueryError(f"{path} must be a finite number")


def load_report(path: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    value, source = read_json(path)
    if not isinstance(value, dict):
        raise QueryError("rotation report must be an object")
    # Profile.rotation is an efficiency summary, not the persisted assignment report.
    if "schema_version" in value:
        raise QueryError("profile JSON is not a rotation report; use the --rotation-out artifact")
    prefix = ""
    source["report_pointer"] = prefix or "/"
    require(value, "profile", str, prefix)
    require(value, "peak_plan", dict, prefix)
    plans = require(value, "assignment_plans", list, prefix)
    if any(not isinstance(plan, dict) for plan in plans):
        raise QueryError("/assignment_plans must contain objects")
    teams = require(value, "teams", list, prefix)
    for i, team in enumerate(teams):
        if not isinstance(team, dict):
            raise QueryError(f"{prefix}/teams/{i} must be an object")
        require(team, "label", str, f"{prefix}/teams/{i}")
        if any(not isinstance(name, str) for name in require(team, "operators", list, f"{prefix}/teams/{i}")):
            raise QueryError(f"{prefix}/teams/{i}/operators must contain strings")
    daily = require(value, "daily", dict, prefix)
    for key in ("trade", "manufacture", "power"):
        number(daily.get(key), f"{prefix}/daily/{key}")
    if "production" in daily:
        production = require(daily, "production", dict, f"{prefix}/daily")
        for key, amount in production.items():
            number(amount, pointer(f"{prefix}/daily/production", key))
    if "warnings" in value and (not isinstance(value["warnings"], list) or
                                any(not isinstance(w, str) for w in value["warnings"])):
        raise QueryError(f"{prefix}/warnings must be an array of strings")
    shifts = require(value, "shifts", list, prefix)
    indices: set[int] = set()
    for i, shift in enumerate(shifts):
        sp = f"{prefix}/shifts/{i}"
        if not isinstance(shift, dict):
            raise QueryError(f"{sp} must be an object")
        index = require(shift, "index", int, sp)
        plan = require(shift, "plan_index", int, sp)
        if index < 0 or index in indices or not 0 <= plan < len(plans):
            raise QueryError(f"{sp}: duplicate/negative index or out-of-range plan_index")
        indices.add(index)
        number(shift.get("duration_hours"), sp + "/duration_hours")
        require(shift, "transition", str, sp)
        require(shift, "resting_team", str, sp)
        if any(not isinstance(t, str) for t in require(shift, "active_teams", list, sp)):
            raise QueryError(f"{sp}/active_teams must contain strings")
        efficiencies = require(shift, "efficiencies", dict, sp)
        for key in ("trade_efficiency", "manufacture_efficiency", "power_efficiency"):
            number(efficiencies.get(key), sp + "/efficiencies/" + key)
        if "room_lines" in efficiencies:
            lines = require(efficiencies, "room_lines", list, sp + "/efficiencies")
            if any(not isinstance(line, dict) or not isinstance(line.get("room_id"), str) for line in lines):
                raise QueryError(f"{sp}/efficiencies/room_lines requires objects with room_id")
        for key in ("weighted_trade", "weighted_manufacture", "weighted_power"):
            number(shift.get(key), pointer(sp, key))
        for key in ("fiammetta", "dorm_rest", "drone"):
            if key in shift and not isinstance(shift[key], dict):
                raise QueryError(f"{sp}/{key} must be an object")
        assignment = require(shift, "assignment", dict, sp)
        rooms = require(assignment, "rooms", list, sp + "/assignment")
        room_ids: set[str] = set()
        for j, room in enumerate(rooms):
            rp = sp + f"/assignment/rooms/{j}"
            if not isinstance(room, dict):
                raise QueryError(f"{rp} must be an object")
            room_id = require(room, "room_id", str, rp)
            if room_id in room_ids:
                raise QueryError(f"{rp}: duplicate room_id {room_id}")
            room_ids.add(room_id)
            for k, operator in enumerate(require(room, "operators", list, rp)):
                op = rp + f"/operators/{k}"
                if not isinstance(operator, dict):
                    raise QueryError(f"{op} must be an object")
                require(operator, "name", str, op)
                for field in ("elite", "level", "rarity"):
                    require(operator, field, int, op)
    return value, source


def summary(report: dict[str, Any], prefix: str, args: Any) -> dict[str, Any]:
    rows = []
    for i, shift in enumerate(report["shifts"]):
        if args.shift is not None and shift["index"] != args.shift - 1:
            continue
        rooms = shift["assignment"]["rooms"]
        matched = [(j, room) for j, room in enumerate(rooms)
                   if (args.room is None or room["room_id"] == args.room) and
                   (args.operator is None or any(o["name"] == args.operator for o in room["operators"]))]
        if (args.room is not None or args.operator is not None) and not matched:
            continue
        sp = f"{prefix}/shifts/{i}"
        row = {"path": sp, "shift": shift["index"] + 1,
               **{k: shift[k] for k in ("index", "plan_index", "duration_hours", "transition",
                                        "active_teams", "resting_team")},
               "plan_path": f"{prefix}/assignment_plans/{shift['plan_index']}",
               "room_count": len(rooms), "matched_room_count": len(matched),
               "efficiencies": {k: v for k, v in shift["efficiencies"].items() if k != "room_lines"}}
        if args.room is not None or args.operator is not None:
            row["rooms"] = [{"path": sp + f"/assignment/rooms/{j}", **room} for j, room in matched]
            row["room_efficiencies"] = [{"path": sp + f"/efficiencies/room_lines/{k}", **x}
                                        for k, x in enumerate(shift["efficiencies"].get("room_lines", []))
                                        if isinstance(x, dict) and x.get("room_id") in
                                        {room["room_id"] for _, room in matched}]
        row["actions"] = {k: shift[k] for k in ("fiammetta", "dorm_rest", "drone") if k in shift}
        rows.append(row)
    warnings = report.get("warnings", [])
    return {"profile": report["profile"], "shift_count": len(report["shifts"]),
            "matched_shift_count": len(rows), "daily": report["daily"],
            "warning_count": len(warnings), "warnings": warnings, "shifts": rows}


def differences(old: Any, new: Any, path: str = "") -> list[dict[str, Any]]:
    if type(old) is not type(new):
        return [{"path": path or "/", "kind": "changed", "before": old, "after": new}]
    changes = []
    if isinstance(old, dict):
        for key in sorted(old.keys() | new.keys()):
            if not path and key == "elapsed":
                continue
            at = pointer(path, key)
            if key not in old:
                changes.append({"path": at, "kind": "added", "after": new[key]})
            elif key not in new:
                changes.append({"path": at, "kind": "removed", "before": old[key]})
            else:
                changes.extend(differences(old[key], new[key], at))
    elif isinstance(old, list):
        for i in range(max(len(old), len(new))):
            at = pointer(path, i)
            if i >= len(old):
                changes.append({"path": at, "kind": "added", "after": new[i]})
            elif i >= len(new):
                changes.append({"path": at, "kind": "removed", "before": old[i]})
            else:
                changes.extend(differences(old[i], new[i], at))
    elif old != new:
        changes.append({"path": path or "/", "kind": "changed", "before": old, "after": new})
    return changes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--compare", type=Path, help="旧报告；按 JSON 路径显示 before/after")
    parser.add_argument("--shift", type=int, help="一基班次编号，匹配保存的 index+1")
    parser.add_argument("--room", help="完整 room_id")
    parser.add_argument("--operator", help="完整干员名")
    add_output_arguments(parser)
    args = parser.parse_args()
    if args.shift is not None and args.shift < 1:
        parser.error("--shift must be positive")
    if args.compare and any(x is not None for x in (args.shift, args.room, args.operator)):
        parser.error("--compare compares complete reports; use a separate query for focused inspection")
    try:
        report, source = load_report(args.report)
        payload: dict[str, Any] = {"schema": "rotation-query-v1", "source": source}
        if args.compare:
            old, previous = load_report(args.compare)
            changes = differences(old, report)
            payload.update(status="ok", previous_source=previous, change_count=len(changes),
                           changes=changes, ignored_paths=["/elapsed"],
                           comparison_scope="decoded rotation JSON; list order preserved")
        else:
            prefix = source["report_pointer"].rstrip("/")
            payload.update(summary(report, prefix, args))
            payload["selection"] = {"shift": args.shift, "room": args.room, "operator": args.operator}
            payload["status"] = "ok" if payload["matched_shift_count"] else "no_match"
        emit(payload, args)
        return int(payload["status"] == "no_match")
    except (QueryError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
