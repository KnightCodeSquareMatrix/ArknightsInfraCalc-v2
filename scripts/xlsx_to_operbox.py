#!/usr/bin/env python3
"""Convert 干员练度表.xlsx to operbox JSON for infra-cli schedule/search."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import pandas as pd


def load_xlsx(path: Path) -> pd.DataFrame:
    df = pd.read_excel(path, sheet_name=0)
    required = ["干员名称", "是否已招募", "星级", "等级", "精英化等级", "潜能等级"]
    missing = [c for c in required if c not in df.columns]
    if missing:
        raise SystemExit(f"missing columns: {missing}")
    return df


def to_operbox_entries(df: pd.DataFrame) -> list[dict]:
    entries: list[dict] = []
    for i, row in df.iterrows():
        own = bool(row["是否已招募"]) if pd.notna(row["是否已招募"]) else False
        elite = int(row["精英化等级"]) if pd.notna(row["精英化等级"]) else 0
        level = int(row["等级"]) if pd.notna(row["等级"]) else 0
        potential = int(row["潜能等级"]) if pd.notna(row["潜能等级"]) else 0
        rarity = int(row["星级"]) if pd.notna(row["星级"]) else 0
        name = str(row["干员名称"]).strip()
        entries.append(
            {
                "id": f"xlsx_{i:04d}",
                "name": name,
                "elite": elite,
                "level": level,
                "own": own,
                "potential": potential,
                "rarity": rarity,
            }
        )
    return entries


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "xlsx",
        type=Path,
        nargs="?",
        default=root / "干员练度表.xlsx",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=root / "data" / "operbox_xlsx.json",
    )
    args = parser.parse_args()

    df = load_xlsx(args.xlsx)
    entries = to_operbox_entries(df)
    owned = sum(1 for e in entries if e["own"])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(entries, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"rows={len(entries)} owned={owned} -> {args.output}")


if __name__ == "__main__":
    main()
