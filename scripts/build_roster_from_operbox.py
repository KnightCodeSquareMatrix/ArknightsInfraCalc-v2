#!/usr/bin/env python3
"""Build roster CSV from an Arknights OperBox export + operator_instances.json."""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path


def tier_key(elite: int) -> str:
    return "tier_up" if elite >= 2 else "tier_0"


def load_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def trade_operators_from_operbox(
    operbox: list[dict],
    instances: dict,
    *,
    owned_only: bool = True,
) -> list[tuple[str, int]]:
    rows: list[tuple[str, int]] = []
    for entry in operbox:
        if owned_only and not entry.get("own"):
            continue
        name = entry["name"]
        elite = int(entry["elite"])
        key = f"{name}@{tier_key(elite)}"
        inst = instances.get("instances", {}).get(key)
        if inst and "trade" in inst.get("facilities", {}):
            rows.append((name, elite))
    rows.sort(key=lambda x: x[0])
    return rows


def write_roster_csv(path: Path, rows: list[tuple[str, int]], facility: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as f:
        w = csv.writer(f)
        w.writerow(["name", "facility", "elite", "buff_ids", "stepwise"])
        for name, elite in rows:
            w.writerow([name, facility, elite, "", "false"])


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operbox", type=Path, help="OperBox export JSON")
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=root / "data" / "roster_gongsun.csv",
    )
    parser.add_argument(
        "--instances",
        type=Path,
        default=root / "data" / "operator_instances.json",
    )
    parser.add_argument(
        "--facility",
        default="trade",
        help="facility column to emit (default: trade)",
    )
    args = parser.parse_args()

    operbox = load_json(args.operbox)
    instances = load_json(args.instances)
    if not isinstance(operbox, list):
        raise SystemExit("operbox JSON must be an array")

    rows = trade_operators_from_operbox(operbox, instances)
    write_roster_csv(args.output, rows, args.facility)
    owned = sum(1 for e in operbox if e.get("own"))
    print(f"owned={owned} {args.facility}_rows={len(rows)} -> {args.output}")


if __name__ == "__main__":
    main()
