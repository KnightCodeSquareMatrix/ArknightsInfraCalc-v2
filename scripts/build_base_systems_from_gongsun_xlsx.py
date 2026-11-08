#!/usr/bin/env python3
"""从公孙长乐「工具人表」xlsx + 练度简表 docx 维护跨设施成套方案目录。

用法：
  uv run python scripts/build_base_systems_from_gongsun_xlsx.py --stdout
  uv run python scripts/build_base_systems_from_gongsun_xlsx.py --merge
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_XLSX = Path.home() / "Downloads" / "工具人表26.5 (2).xlsx"
OUT_PATH = ROOT / "data" / "base_systems.json"

# 与 data/base_systems.json 同步：docx「大组」+ xlsx 跨设施行
CURATED_SYSTEMS = [
    {
        "id": "rosemary_perception",
        "label": "迷迭香感知链：黑键贸+夕中枢+感知宿舍+迷迭香赤金（243-高配3班 peak）",
        "priority": 18,
        "shift_modes": ["peak"],
        "xlsx_hint": "243-高配3班 peak：黑键+迷迭香+夕+感知宿舍",
        "slots": [
            {
                "facility": "trade_post",
                "room_id": "trade_1",
                "operators": [
                    {"name": "黑键", "elite": 2},
                    {"name": "吉星", "elite": 2},
                    {"name": "可露希尔", "elite": 2},
                ],
            },
            {"facility": "control", "operators": [{"name": "夕", "elite": 2}]},
            {
                "facility": "dormitory",
                "room_id": "dorm_1",
                "operators": [
                    {"name": "车尔尼", "elite": 2},
                    {"name": "爱丽丝", "elite": 2},
                    {"name": "塑心", "elite": 2},
                ],
            },
            {
                "facility": "factory",
                "room_id": "manu_4",
                "operators": [
                    {"name": "阿罗玛", "elite": 2},
                    {"name": "砾", "elite": 2},
                    {"name": "迷迭香", "elite": 2},
                ],
            },
        ],
    },
    {
        "id": "witch_long_beta",
        "label": "巫恋组：巫恋Ⅱ+龙舌兰Ⅱ+裁缝β（138%贸+46%金）",
        "priority": 16,
        "shortcut_id": "gsl_witch_long_beta",
        "shift_modes": ["peak"],
        "xlsx_hint": "巫恋Ⅱ+龙舌兰Ⅱ+裁缝β",
        "slots": [
            {
                "facility": "trade_post",
                "trade_role": "meta_witch",
                "operators": [
                    {"name": "巫恋", "elite": 2},
                    {"name": "龙舌兰", "elite": 2},
                    {"pick_one": ["卡夫卡", "柏喙", "明椒", "折光"], "elite": 2},
                ],
            },
        ],
    },
    {
        "id": "lungmen_manu_pair",
        "label": "龙门组中枢：斩业星熊+诗怀雅（全制造+3%）",
        "priority": 14,
        "shift_modes": ["peak"],
        "xlsx_hint": "龙门组 斩业星熊｛诗怀雅｝",
        "slots": [
            {
                "facility": "control",
                "operators": [
                    {"name": "斩业星熊", "elite": 2},
                    {"name": "诗怀雅", "elite": 0},
                ],
            },
        ],
    },
    {
        "id": "snhunt_monhun_control",
        "label": "怪猎中枢：火龙S黑角+麒麟R夜刀（木天蓼；需调查团消费方）",
        "tier": "cross_station",
        "priority": 11,
        "exclusive_group": "control_global_effect",
        "shift_modes": ["peak"],
        "bind_all": True,
        "xlsx_hint": "泰拉大陆调查团(+怪猎中枢)",
        "slots": [
            {
                "facility": "control",
                "operators": [
                    {"name": "火龙S黑角", "elite": 0},
                    {"name": "麒麟R夜刀", "elite": 0},
                ],
            },
            {
                "facility": "trade_post",
                "fill": "search",
                "required_count": 1,
                "operators": [{"name": "泰拉大陆调查团", "elite": 0}],
            },
        ],
    },
    {
        "id": "snhunt_monhun_control_factory",
        "label": "怪猎中枢制造消费：火龙S黑角+麒麟R夜刀+调查团",
        "tier": "cross_station",
        "priority": 11,
        "exclusive_group": "control_global_effect",
        "shift_modes": ["peak"],
        "bind_all": True,
        "xlsx_hint": "泰拉大陆调查团(+怪猎中枢)",
        "slots": [
            {
                "facility": "control",
                "operators": [
                    {"name": "火龙S黑角", "elite": 0},
                    {"name": "麒麟R夜刀", "elite": 0},
                ],
            },
            {
                "facility": "factory",
                "operators": [
                    {"name": "泰拉大陆调查团", "elite": 0, "min_level": 30},
                ],
            },
        ],
    },
]

FORBIDDEN_FIXED_SYRACUSA_SYSTEM_IDS = {
    "docus_syracusa",
    "syracusa_pair",
    "syracusa_cross_station",
}


def scan_xlsx_hints(xlsx: Path) -> list[str]:
    import openpyxl

    wb = openpyxl.load_workbook(xlsx, read_only=True, data_only=True)
    ws = wb["Sheet1"]
    hints: list[str] = []
    pat = re.compile(r"中枢|伺夜|贝洛内|灵知|孑|银灰|巫恋|龙舌兰|戴菲恩|维娜|调查团|怪猎")
    for row in ws.iter_rows(values_only=True):
        cells = [str(c).strip() for c in row if c is not None and str(c).strip()]
        line = " ".join(cells)
        if pat.search(line):
            hints.append(line[:120])
    wb.close()
    return hints


def build_document(xlsx: Path) -> dict:
    hints = scan_xlsx_hints(xlsx)
    systems = [{k: v for k, v in entry.items() if k != "xlsx_hint"} for entry in CURATED_SYSTEMS]
    control_manu_injectors = [
        {
            "id": "lungmen_starbear_shixy",
            "label": "龙门组：斩业星熊+诗怀雅（全制造+3%，同族取最高）",
            "manu_all_pct": 3,
            "operators": [
                {"name": "斩业星熊", "elite": 2},
                {"name": "诗怀雅", "elite": 0},
            ],
        },
        {
            "id": "mon3tr",
            "label": "Mon3tr E2 最高权限（全制造+2%，凯尔希平替）",
            "manu_all_pct": 2,
            "operators": [{"name": "Mon3tr", "elite": 2}],
        },
        {
            "id": "kaltsit",
            "label": "凯尔希 E2 最高权限（全制造+2%）",
            "manu_all_pct": 2,
            "operators": [{"name": "凯尔希", "elite": 2}],
        },
        {
            "id": "kirin_yato_e2",
            "label": "麒麟R夜刀 E2 以身作则（全制造+2%，需同房怪猎干员）",
            "manu_all_pct": 2,
            "operators": [{"name": "麒麟R夜刀", "elite": 2}],
            "requires_monhun_peer": True,
        },
    ]
    document = {
        "version": 1,
        "source": f"{xlsx.name} + 基建干员练度简表 + 排班迭代说明 @公孙长乐",
        "control_manu_injectors": control_manu_injectors,
        "xlsx_scan_hints": hints[:25],
        "systems": systems,
    }
    validate_document(document)
    return document


def validate_document(document: dict) -> None:
    system_ids = {entry.get("id") for entry in document.get("systems", [])}
    forbidden = sorted(system_ids & FORBIDDEN_FIXED_SYRACUSA_SYSTEM_IDS)
    if forbidden:
        raise ValueError(f"fixed Syracusa systems are forbidden: {forbidden}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--xlsx", type=Path, default=DEFAULT_XLSX)
    ap.add_argument("--merge", action="store_true", help="写入 data/base_systems.json")
    ap.add_argument("--stdout", action="store_true", help="只打印 JSON")
    args = ap.parse_args()
    if not args.xlsx.is_file():
        raise SystemExit(f"xlsx not found: {args.xlsx}")

    doc = build_document(args.xlsx)
    if args.stdout or not args.merge:
        preview = doc if args.stdout else {k: v for k, v in doc.items() if k != "xlsx_scan_hints"}
        print(json.dumps(preview, ensure_ascii=False, indent=2))
        if not args.merge:
            import sys

            print(f"\n# scanned {len(doc['xlsx_scan_hints'])} hint lines", file=sys.stderr)
        return

    out = {k: v for k, v in doc.items() if k != "xlsx_scan_hints"}
    OUT_PATH.write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {OUT_PATH}")


if __name__ == "__main__":
    main()
