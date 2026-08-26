#!/usr/bin/env python3
"""Audit Karland admission and final rotation cohesion without using MAA/text output."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GROUPS = {
    "representative": ROOT / "data/fixtures/box-bundles/collected/collected_operboxes",
    "high-owned": ROOT / "data/fixtures/box-bundles/high-owned",
}
DEFAULT_LAYOUT = ROOT / "data/fixtures/243/layout.json"
KARLAN_SYSTEM_ID = "ling_jie_karlan"


def selected(operator: dict[str, object], name: str, minimum_elite: int) -> bool:
    return operator.get("name") == name and int(operator.get("elite", 0)) >= minimum_elite


def owns(operator_box: list[dict[str, object]], name: str, minimum_elite: int) -> bool:
    return any(bool(operator.get("own")) and selected(operator, name, minimum_elite) for operator in operator_box)


def trade_room_ids(layout_path: Path) -> set[str]:
    layout = json.loads(layout_path.read_text())
    return {
        str(room["id"])
        for room in layout["rooms"]
        if room["kind"] == "trade_post"
    }


def karlan_missing_control_source(rotation: dict[str, object], trade_rooms: set[str]) -> list[str]:
    failures: list[str] = []
    for shift in rotation["shifts"]:
        assignment = shift["assignment"]
        rooms = assignment["rooms"]
        control = next((room["operators"] for room in rooms if room["room_id"] == "control"), [])
        has_gnosis = any(selected(operator, "灵知", 2) for operator in control)
        for room in rooms:
            if room["room_id"] not in trade_rooms:
                continue
            operators = room["operators"]
            has_silverash = any(selected(operator, "银灰", 2) for operator in operators)
            has_jaye = any(selected(operator, "孑", 1) for operator in operators)
            if has_silverash and has_jaye and not has_gnosis:
                control_names = ", ".join(operator["name"] for operator in control) or "<empty>"
                failures.append(
                    f"shift {int(shift['index']) + 1} room {room['room_id']}: "
                    f"control=[{control_names}]"
                )
    return failures


def claimed_karlan_members(rotation: dict[str, object]) -> tuple[set[str], set[str]] | None:
    claims = rotation["peak_plan"]["registry_claims"]
    claim = next((claim for claim in claims if claim["system_id"] == KARLAN_SYSTEM_ID), None)
    if claim is None:
        return None

    control_members: set[str] = set()
    trade_members: set[str] = set()
    for slot in claim["slots"]:
        names = {operator["name"] for operator in slot["operators"]}
        if slot["facility"] == "control_center":
            control_members.update(names)
        elif slot["facility"] == "trade_post":
            trade_members.update(names)
    return control_members, trade_members


def karlan_claim_cohort_failures(
    rotation: dict[str, object], trade_rooms: set[str]
) -> list[str]:
    claimed = claimed_karlan_members(rotation)
    if claimed is None:
        return []

    control_members, trade_members = claimed
    required_members = control_members | trade_members
    if not control_members or not trade_members:
        return ["peak claim has no resolved control or trade members"]

    failures: list[str] = []
    active_shifts = 0
    for shift in rotation["shifts"]:
        rooms = shift["assignment"]["rooms"]
        present = {operator["name"] for room in rooms for operator in room["operators"]}
        active_members = present & required_members
        if not active_members:
            continue
        active_shifts += 1
        missing = required_members - present
        if missing:
            failures.append(
                f"shift {int(shift['index']) + 1}: claimed cohort split, missing={sorted(missing)}"
            )
            continue
        control = next((room["operators"] for room in rooms if room["room_id"] == "control"), [])
        control_names = {operator["name"] for operator in control}
        if not control_members <= control_names:
            failures.append(
                f"shift {int(shift['index']) + 1}: control missing={sorted(control_members - control_names)}"
            )
        elif not any(
            trade_members <= {operator["name"] for operator in room["operators"]}
            for room in rooms
            if room["room_id"] in trade_rooms
        ):
            failures.append(
                f"shift {int(shift['index']) + 1}: trade cohort split={sorted(trade_members)}"
            )

    claim = next(
        claim
        for claim in rotation["peak_plan"]["registry_claims"]
        if claim["system_id"] == KARLAN_SYSTEM_ID
    )
    expected_active = int(claim.get("on_shifts", 0))
    if expected_active and active_shifts != expected_active:
        failures.append(f"claimed cohort active {active_shifts} shifts; expected {expected_active}")
    return failures


def failure_category(stderr: str) -> str:
    reason = stderr.strip().splitlines()[-1] if stderr.strip() else "failed"
    if "assert-invariants" in reason:
        return "invariant"
    if "operbox parse" in reason or "invalid type" in reason:
        return "input"
    return "solver"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Check final rotations for SilverAsh E2 + Jaye E1+ without Gnosis E2."
    )
    parser.add_argument("--group", choices=["representative", "high-owned", "all"], default="all")
    parser.add_argument("--layout", type=Path, default=DEFAULT_LAYOUT)
    parser.add_argument("--rotation", default="3")
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--assert-invariants", action="store_true")
    args = parser.parse_args()

    trade_rooms = trade_room_ids(args.layout)
    groups = GROUPS.items() if args.group == "all" else [(args.group, GROUPS[args.group])]
    binary = ROOT / "target/debug/infra-cli"
    subprocess.run(["cargo", "build", "-p", "infra-cli"], cwd=ROOT, check=True)

    failures: dict[str, list[tuple[str, str, str]]] = {
        "karland_missing_control": [],
        "karland_claim_cohort": [],
        "invariant": [],
        "solver": [],
        "input": [],
        "timeout": [],
    }
    reviews: list[tuple[str, str, str]] = []
    total = 0
    completed = 0
    pair_shifts = 0
    qualified_silverash_trios = 0
    claimed_karlan_chains = 0
    for group, directory in groups:
        boxes = sorted(path for path in directory.glob("*.json") if not path.stem.endswith("_profile"))
        for box_path in boxes:
            total += 1
            try:
                operator_box = json.loads(box_path.read_text())
            except (OSError, json.JSONDecodeError) as error:
                failures["input"].append((group, box_path.name, f"operbox unavailable: {error}"))
                continue
            with tempfile.TemporaryDirectory() as temporary:
                rotation_path = Path(temporary) / "rotation.json"
                command = [
                    str(binary),
                    "plan",
                    "--layout",
                    str(args.layout),
                    "--operbox",
                    str(box_path),
                    "--rotation",
                    args.rotation,
                    "--profile-out",
                    str(Path(temporary) / "profile.json"),
                    "--rotation-out",
                    str(rotation_path),
                ]
                if args.assert_invariants:
                    command.append("--assert-invariants")
                try:
                    result = subprocess.run(
                        command,
                        cwd=ROOT,
                        capture_output=True,
                        text=True,
                        timeout=args.timeout,
                    )
                except subprocess.TimeoutExpired:
                    failures["timeout"].append(
                        (group, box_path.name, f"timeout after {args.timeout}s")
                    )
                    continue
                if result.returncode:
                    category = failure_category(result.stderr)
                    reason = result.stderr.strip().splitlines()[-1] if result.stderr.strip() else "failed"
                    failures[category].append((group, box_path.name, reason))
                    continue
                try:
                    rotation = json.loads(rotation_path.read_text())
                except (OSError, json.JSONDecodeError) as error:
                    failures["input"].append(
                        (group, box_path.name, f"rotation JSON unavailable: {error}")
                    )
                    continue

            completed += 1
            found = karlan_missing_control_source(rotation, trade_rooms)
            claimed = claimed_karlan_members(rotation)
            if claimed is not None:
                claimed_karlan_chains += 1
                failures["karland_claim_cohort"].extend(
                    (group, box_path.name, detail)
                    for detail in karlan_claim_cohort_failures(rotation, trade_rooms)
                )
            if (
                owns(operator_box, "灵知", 2)
                and owns(operator_box, "孑", 1)
                and owns(operator_box, "银灰", 2)
            ):
                qualified_silverash_trios += 1
                if claimed is None:
                    reviews.append(
                        (
                            group,
                            box_path.name,
                            "qualified 灵知 E2 + 孑 E1+ + 银灰 E2 but peak did not claim ling_jie_karlan",
                        )
                    )
            for shift in rotation["shifts"]:
                rooms = shift["assignment"]["rooms"]
                pair_present = False
                for room in rooms:
                    if room["room_id"] not in trade_rooms:
                        continue
                    operators = room["operators"]
                    if any(selected(operator, "银灰", 2) for operator in operators) and any(
                        selected(operator, "孑", 1) for operator in operators
                    ):
                        pair_present = True
                pair_shifts += int(pair_present)
            failures["karland_missing_control"].extend(
                (group, box_path.name, detail) for detail in found
            )

    failed = sum(len(items) for items in failures.values())
    print(
        f"checked={total} completed={completed} silverash_jaye_trade_shifts={pair_shifts} "
        f"qualified_silverash_trios={qualified_silverash_trios} "
        f"claimed_karlan_chains={claimed_karlan_chains} failed={failed}"
    )
    for category, items in failures.items():
        print(f"{category}_failures={len(items)}")
        for group, name, reason in items:
            print(f"FAIL {category} {group}/{name}: {reason}")
    print(f"unclaimed_eligible_reviews={len(reviews)}")
    for group, name, reason in reviews:
        print(f"REVIEW unclaimed_eligible {group}/{name}: {reason}")
    return int(bool(failed))


if __name__ == "__main__":
    sys.exit(main())
