#!/usr/bin/env python3
"""Audit operbox elite+rarity vs tier_0/tier_up manufacture bindings."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def tier_threshold(rarity: int) -> int:
    if rarity <= 2:
        return 99  # 1-2★: use level>=30, not elite
    if rarity <= 4:
        return 1  # 3-4★: tier_up at E1
    return 2  # 5-6★: tier_up at E2


def should_tier_up(elite: int, rarity: int, level: int = 0) -> bool:
    if rarity <= 2:
        return level >= 30
    return elite >= tier_threshold(rarity)


def rust_current(elite: int) -> bool:
    return elite >= 2


def main() -> None:
    operbox = [
        e
        for e in json.loads(
            (ROOT / "data" / "operbox_xlsx_new.json").read_text(encoding="utf-8")
        )
        if e["own"]
    ]
    instances = json.loads(
        (ROOT / "data" / "operator_instances.json").read_text(encoding="utf-8")
    )["instances"]

    def has_manu(name: str, tier: str) -> bool:
        inst = instances.get(f"{name}@{tier}")
        return bool(inst and inst.get("facilities", {}).get("manufacture"))

    wrong_rust = []
    wrong_if_naive_e1 = []

    for e in operbox:
        name, elite, rarity = e["name"], e["elite"], e["rarity"]
        correct = should_tier_up(elite, rarity, e.get("level", 0))
        rust = rust_current(elite)
        naive = elite >= 1 and rarity >= 3

        t0, tu = has_manu(name, "tier_0"), has_manu(name, "tier_up")
        if not t0 and not tu:
            continue

        if correct != rust:
            wrong_rust.append((name, elite, rarity, correct, rust, t0, tu))
        if correct != naive and naive != rust:
            wrong_if_naive_e1.append((name, elite, rarity, correct, naive))

    print("=== Rust current (elite>=2 -> tier_up) mismatches ===")
    print(f"count={len(wrong_rust)}")
    for row in wrong_rust:
        name, elite, rarity, correct, rust, t0, tu = row
        print(
            f"  {name} {rarity}★ e{elite}  correct_tier_up={correct} rust={rust}  "
            f"manu@t0={t0} manu@up={tu}"
        )

    print()
    print("=== Would break if naive fix (elite>=1 for 3★+) ===")
    print(f"count={len(wrong_if_naive_e1)}")
    for row in wrong_if_naive_e1[:15]:
        name, elite, rarity, correct, naive = row
        print(f"  {name} {rarity}★ e{elite}  correct={correct} naive={naive}")
    if len(wrong_if_naive_e1) > 15:
        print(f"  ... +{len(wrong_if_naive_e1)-15} more")


if __name__ == "__main__":
    main()
