#!/usr/bin/env python3
"""Read-only audit for data/MECHANICS_REGISTRY.csv.

This script intentionally does not generate executable rules. It tags the
registry text, compares it with current runtime data at a coarse level, and
writes a Markdown report for the 90 -> 95 quality-improvement work.
"""

from __future__ import annotations

import argparse
import csv
import json
import re
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"
DEFAULT_OUTPUT = ROOT / "docs" / "INTERNAL" / "MECHANICS_REGISTRY_AUDIT.md"

FACILITY_MAP = {
    "贸易站": "trade",
    "制造站": "manufacture",
    "发电站": "power",
    "控制中枢": "control",
    "宿舍": "dorm",
    "办公室": "office",
    "会客室": "reception",
    "加工站": "workshop",
    "训练室": "training",
}

RUNTIME_FACILITIES = {"trade", "manufacture", "power", "control", "dorm", "office"}

TAG_RULES: list[tuple[str, list[str]]] = [
    ("pair_synergy", ["当与", "一起工作", "同时进驻"]),
    ("same_room_synergy", ["同一个", "同一", "每名进驻在该", "该制造站", "该贸易站"]),
    ("cross_room_synergy", ["进驻在制造站", "进驻在贸易站", "进驻在发电站", "进驻在控制中枢", "每个贸易站", "每间贸易站", "每个制造站"]),
    ("global_bonus", ["基建内", "全基建", "所有制造站", "所有贸易站", "所有设施"]),
    ("recipe_specific_bonus", ["制造赤金", "制造战斗记录", "制造源石", "赤金", "战斗记录", "源石", "贵金属订单", "龙门商法"]),
    ("faction_count_bonus", ["黑钢国际", "乌萨斯学生自治团", "深海猎人", "莱茵生命", "红松骑士团", "企鹅物流", "格拉斯哥帮", "喀兰贸易", "叙拉古"]),
    ("mood_recovery", ["心情每小时恢复", "恢复+"]),
    ("mood_consumption", ["心情每小时消耗", "消耗+", "消耗-"]),
    ("capacity_bonus", ["仓库容量", "容量上限", "订单上限"]),
    ("cap_rule", ["上限", "最高", "不超过"]),
    ("max_of_same_effect", ["同种效果取最高"]),
    ("conditional_if", ["如果", "当自身", "心情大于", "心情处于", "低于", "高于"]),
    ("threshold", ["每", "大于", "低于", "达到", "每有", "每个", "每间"]),
    ("negative_effect", ["降低", "减少", "消耗+", "清空", "降为", "-"]),
    ("special_stacking", ["额外", "特殊", "叠加", "转化", "转换", "变为"]),
    ("resource_token", ["热情值", "人间烟火", "感知信息", "魔物料理", "木天蓼", "巫术结晶"]),
]

HIGH_RISK_TERMS = [
    "当与",
    "每个",
    "如果",
    "基建内",
    "额外",
    "上限",
    "同种效果取最高",
    "特殊",
    "制造站",
    "贸易站",
    "发电站",
    "控制中枢",
    "热情值",
    "人间烟火",
    "感知信息",
]

SYSTEM_TERMS = [
    "清流",
    "温蒂",
    "冬时",
    "承曦格雷伊",
    "森蚺",
    "红云",
    "酒神",
    "Miss.Christine",
    "水月",
    "海沫",
    "标准化",
    "迷迭香",
    "黑键",
    "乌有",
    "森西",
    "伺夜",
    "贝洛内",
    "但书",
    "可露希尔",
    "巫恋",
    "龙舌兰",
]


@dataclass(frozen=True)
class RegistryRow:
    index: str
    skill_name: str
    facility_cn: str
    product: str
    operator: str
    elite: str
    efficiency: str
    text: str

    @property
    def facility(self) -> str:
        return FACILITY_MAP.get(self.facility_cn, self.facility_cn)


def load_registry(path: Path) -> list[RegistryRow]:
    with path.open(encoding="utf-8-sig", newline="") as f:
        rows = []
        for row in csv.DictReader(f):
            rows.append(
                RegistryRow(
                    index=row["序号"].strip(),
                    skill_name=row["技能名"].strip(),
                    facility_cn=row["工作设施"].strip(),
                    product=row["产物限定"].strip(),
                    operator=row["干员"].strip(),
                    elite=row["需求精英"].strip(),
                    efficiency=row["效率值"].strip(),
                    text=row["游戏原文"].strip(),
                )
            )
    return rows


def load_skill_table() -> dict[str, list[dict]]:
    skills = json.loads((DATA / "skill_table.json").read_text(encoding="utf-8"))["skills"]
    by_key: dict[str, list[dict]] = defaultdict(list)
    for skill in skills:
        by_key[f"{skill.get('facility')}::{skill.get('skill_name')}"].append(skill)
    return by_key


def load_instance_pairs() -> set[tuple[str, str]]:
    instances = json.loads((DATA / "operator_instances.json").read_text(encoding="utf-8"))[
        "instances"
    ]
    pairs: set[tuple[str, str]] = set()
    for row in instances.values():
        name = row["name"]
        for facility in row.get("facilities", {}):
            pairs.add((name, facility))
    return pairs


def load_system_operators() -> dict[str, set[str]]:
    systems = json.loads((DATA / "base_systems.json").read_text(encoding="utf-8"))
    out: dict[str, set[str]] = {}
    for system in systems.get("systems", []):
        ops: set[str] = set()
        for slot in system.get("slots", []):
            for op in slot.get("operators", []):
                if isinstance(op, dict) and op.get("name"):
                    ops.add(op["name"])
            for op in slot.get("pick_one", []):
                if isinstance(op, str):
                    ops.add(op)
                elif isinstance(op, dict) and op.get("name"):
                    ops.add(op["name"])
        out[system["id"]] = ops
    return out


def tags_for(row: RegistryRow) -> list[str]:
    text = row.text
    tags = [name for name, terms in TAG_RULES if any(term in text for term in terms)]
    if row.efficiency:
        tags.append("flat_bonus")
    # Keep the taxonomy bucket visible even when there is no obvious tag.
    return sorted(set(tags))


def risk_terms_for(row: RegistryRow) -> list[str]:
    text = row.text
    return [term for term in HIGH_RISK_TERMS if term in text]


def actionable_risk_terms_for(row: RegistryRow) -> list[str]:
    # Facility names are useful in aggregate counts, but "制造站" in a
    # manufacture skill is not by itself actionable. Keep cross-facility names.
    own_facility = row.facility_cn
    return [term for term in risk_terms_for(row) if term != own_facility]


def skill_table_status(row: RegistryRow, skills_by_key: dict[str, list[dict]]) -> str:
    if row.facility not in RUNTIME_FACILITIES:
        return "out_of_runtime_scope"
    skills = skills_by_key.get(f"{row.facility}::{row.skill_name}", [])
    if not skills:
        return "not_found_by_name"
    if any(skill.get("atoms") for skill in skills):
        return "has_atoms"
    return "empty_atoms"


def instance_status(row: RegistryRow, instance_pairs: set[tuple[str, str]]) -> str:
    if row.facility not in RUNTIME_FACILITIES:
        return "out_of_runtime_scope"
    return "bound" if (row.operator, row.facility) in instance_pairs else "not_bound"


def md_escape(text: str) -> str:
    return text.replace("|", "\\|").replace("\n", " ")


def truncate(text: str, limit: int = 96) -> str:
    text = re.sub(r"\s+", " ", text).strip()
    return text if len(text) <= limit else text[: limit - 1] + "..."


def table(headers: list[str], rows: list[list[object]]) -> list[str]:
    lines = ["| " + " | ".join(headers) + " |", "| " + " | ".join(["---"] * len(headers)) + " |"]
    for row in rows:
        lines.append("| " + " | ".join(md_escape(str(cell)) for cell in row) + " |")
    return lines


def bullet_counts(counter: Counter[str]) -> list[str]:
    return [f"- `{key}`: {value}" for key, value in counter.most_common()]


def build_report(rows: list[RegistryRow]) -> str:
    skills_by_key = load_skill_table()
    instance_pairs = load_instance_pairs()
    system_ops = load_system_operators()

    row_tags = {row.index: tags_for(row) for row in rows}
    row_risks = {row.index: risk_terms_for(row) for row in rows}
    row_actionable_risks = {row.index: actionable_risk_terms_for(row) for row in rows}
    skill_status = {row.index: skill_table_status(row, skills_by_key) for row in rows}
    inst_status = {row.index: instance_status(row, instance_pairs) for row in rows}

    facility_counts = Counter(row.facility_cn for row in rows)
    tag_counts = Counter(tag for tags in row_tags.values() for tag in tags)
    risk_counts = Counter(term for terms in row_risks.values() for term in terms)
    skill_counts = Counter(skill_status.values())
    inst_counts = Counter(inst_status.values())

    runtime_rows = [row for row in rows if row.facility in RUNTIME_FACILITIES]
    manu_rows = [row for row in rows if row.facility == "manufacture"]
    manu_risky = sorted(
        [
            row
            for row in manu_rows
            if row_actionable_risks[row.index]
            or {"pair_synergy", "same_room_synergy", "cross_room_synergy", "global_bonus", "resource_token"}
            & set(row_tags[row.index])
        ],
        key=lambda r: (-len(row_risks[r.index]), r.operator, r.skill_name),
    )
    cross_rows = [
        row
        for row in runtime_rows
        if {"cross_room_synergy", "global_bonus", "resource_token"} & set(row_tags[row.index])
    ]
    system_candidates = [
        row
        for row in runtime_rows
        if row_risks[row.index]
        and (
            {"pair_synergy", "same_room_synergy", "cross_room_synergy", "global_bonus", "resource_token"} & set(row_tags[row.index])
            or any(term in row.text or term in row.operator or term in row.skill_name for term in SYSTEM_TERMS)
        )
    ]
    feedback_candidates = [
        row
        for row in runtime_rows
        if any(term in row.text or term in row.operator or term in row.skill_name for term in SYSTEM_TERMS)
    ]

    model_gaps = [
        row
        for row in runtime_rows
        if skill_status[row.index] in {"not_found_by_name", "empty_atoms"} or inst_status[row.index] == "not_bound"
    ]
    high_risk_gaps = [
        row
        for row in model_gaps
        if row_actionable_risks[row.index]
        or {"cross_room_synergy", "global_bonus", "resource_token", "pair_synergy"} & set(row_tags[row.index])
    ]

    system_hits: list[list[object]] = []
    for system_id, ops in sorted(system_ops.items()):
        hits = [row for row in rows if row.operator in ops]
        if not hits:
            continue
        risky_count = sum(
            1
            for row in hits
            if row_actionable_risks[row.index]
            or {
                "cross_room_synergy",
                "global_bonus",
                "resource_token",
                "pair_synergy",
                "same_room_synergy",
            }
            & set(row_tags[row.index])
        )
        system_hits.append(
            [
                system_id,
                ", ".join(sorted(ops)),
                len(hits),
                risky_count,
                ", ".join(sorted({row.facility_cn for row in hits})),
            ]
        )

    lines: list[str] = []
    append = lines.append
    append("# MECHANICS_REGISTRY Audit")
    append("")
    append("> Generated by `python3 scripts/audit_mechanics_registry.py`. This is a read-only report: rows marked as candidates still require manual confirmation before entering runtime rules.")
    append("")
    append("## Summary")
    append("")
    append(f"- Registry rows: {len(rows)}")
    append(f"- Runtime-scope rows: {len(runtime_rows)} (`trade`, `manufacture`, `power`, `control`, `dorm`, `office`)")
    append(f"- Manufacture rows: {len(manu_rows)}")
    append(f"- Manufacture risk rows: {len(manu_risky)}")
    append(f"- Cross-facility / global candidate rows: {len(cross_rows)}")
    append(f"- High-risk runtime model-gap rows: {len(high_risk_gaps)}")
    append("")
    append("## Facility Counts")
    append("")
    lines.extend(bullet_counts(facility_counts))
    append("")
    append("## Runtime Coverage Snapshot")
    append("")
    append("This is a coarse name/facility audit. It is useful for triage, not proof of semantic coverage.")
    append("")
    append("Skill-table status:")
    lines.extend(bullet_counts(skill_counts))
    append("")
    append("Operator-instance status:")
    lines.extend(bullet_counts(inst_counts))
    append("")
    append("## Tag Taxonomy Counts")
    append("")
    lines.extend(bullet_counts(tag_counts))
    append("")
    append("## High-Risk Keyword Counts")
    append("")
    lines.extend(bullet_counts(risk_counts))
    append("")
    append("## Manufacture Risk Rows")
    append("")
    lines.extend(
        table(
            ["#", "operator", "skill", "product", "tags", "risk terms", "skill status", "text"],
            [
                [
                    row.index,
                    row.operator,
                    row.skill_name,
                    row.product or "-",
                    ", ".join(row_tags[row.index]) or "-",
                    ", ".join(row_actionable_risks[row.index]) or "-",
                    skill_status[row.index],
                    truncate(row.text),
                ]
                for row in manu_risky[:40]
            ],
        )
    )
    append("")
    append("## Cross-Facility / Global Candidate Rows")
    append("")
    lines.extend(
        table(
            ["#", "facility", "operator", "skill", "tags", "risk terms", "text"],
            [
                [
                    row.index,
                    row.facility_cn,
                    row.operator,
                    row.skill_name,
                    ", ".join(row_tags[row.index]) or "-",
                    ", ".join(row_actionable_risks[row.index]) or "-",
                    truncate(row.text),
                ]
                for row in cross_rows[:60]
            ],
        )
    )
    append("")
    append("## System Candidate Discovery")
    append("")
    append("Rows below are candidates for human review only. They are selected by coupling tags, high-risk terms, and known system/operator names.")
    append("")
    lines.extend(
        table(
            ["#", "facility", "operator", "skill", "tags", "risk terms", "text"],
            [
                [
                    row.index,
                    row.facility_cn,
                    row.operator,
                    row.skill_name,
                    ", ".join(row_tags[row.index]) or "-",
                    ", ".join(row_risks[row.index]) or "-",
                    truncate(row.text),
                ]
                for row in system_candidates[:60]
            ],
        )
    )
    append("")
    append("## Existing base_systems Overlap")
    append("")
    lines.extend(
        table(
            ["system_id", "operators", "registry rows", "risk rows", "facilities"],
            system_hits,
        )
    )
    append("")
    append("## Feedback-Oriented Regression Seeds")
    append("")
    append("These rows mention operators or mechanics seen in recent feedback. Suggested next step: turn selected rows into preferred/forbidden pattern tests, not executable formulas.")
    append("")
    lines.extend(
        table(
            ["#", "facility", "operator", "skill", "tags", "risk terms", "text"],
            [
                [
                    row.index,
                    row.facility_cn,
                    row.operator,
                    row.skill_name,
                    ", ".join(row_tags[row.index]) or "-",
                    ", ".join(row_risks[row.index]) or "-",
                    truncate(row.text),
                ]
                for row in feedback_candidates[:60]
            ],
        )
    )
    append("")
    append("## High-Risk Model Gaps")
    append("")
    append("These rows are runtime-scope entries with coarse coverage gaps and coupling/high-risk signals. Do not fix formulas from this table alone; use it to pick manual audit targets.")
    append("")
    lines.extend(
        table(
            ["#", "facility", "operator", "skill", "skill status", "instance status", "tags", "risk terms", "text"],
            [
                [
                    row.index,
                    row.facility_cn,
                    row.operator,
                    row.skill_name,
                    skill_status[row.index],
                    inst_status[row.index],
                    ", ".join(row_tags[row.index]) or "-",
                    ", ".join(row_risks[row.index]) or "-",
                    truncate(row.text),
                ]
                for row in high_risk_gaps[:80]
            ],
        )
    )
    append("")
    append("## Test Suggestions")
    append("")
    append("- Convert `feedback/2026-06-29/010315-推荐调整成清流温蒂冬时-挂钩发电承曦格雷伊-130` into a manufacture candidate-regression seed: preferred pattern `清流 + 温蒂 + 冬时`, linked producer `承曦格雷伊`, and max regret tolerance.")
    append("- Add a trace expectation that explains whether a system candidate was selected by near-optimal preference or rejected by raw-score gap/operator conflict.")
    append("- Use the high-risk model-gap table to choose manual audits for cross-room/global tokens before adding any runtime rule.")
    append("")
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, default=DATA / "MECHANICS_REGISTRY.csv")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()

    rows = load_registry(args.registry)
    report = build_report(rows)
    output = args.output if args.output.is_absolute() else ROOT / args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(report, encoding="utf-8")
    print(f"wrote {output.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
