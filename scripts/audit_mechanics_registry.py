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
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"
DEFAULT_OUTPUT = ROOT / "docs" / "INTERNAL" / "MECHANICS_REGISTRY_AUDIT.md"
DEFAULT_JSON_OUTPUT = ROOT / "target" / "generated" / "mechanics_registry_audit_full.json"

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
    ("special_stacking", ["额外", "特殊", "叠加", "转化", "转换", "变为"]),
    ("resource_token", ["热情值", "人间烟火", "感知信息", "魔物料理", "木天蓼", "巫术结晶"]),
]

REGEX_TAG_RULES: list[tuple[str, list[re.Pattern[str]]]] = [
    (
        "negative_effect",
        [
            re.compile(r"生产力-\d"),
            re.compile(r"效率-\d"),
            re.compile(r"心情每小时消耗\+\d"),
            re.compile(r"清空"),
            re.compile(r"降为"),
            re.compile(r"减少"),
            re.compile(r"降低"),
        ],
    ),
    ("count_based", [re.compile(r"每[个名间有1一]"), re.compile(r"每\d+")]),
    ("ratio_conversion", [re.compile(r"每\d+.*转化为"), re.compile(r"每有?\d+.*则")]),
    ("threshold_gate", [re.compile(r"大于"), re.compile(r"低于"), re.compile(r"达到"), re.compile(r"处于")]),
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


def load_known_operator_names(registry_rows: list[RegistryRow]) -> set[str]:
    names: set[str] = set()
    instances = json.loads((DATA / "operator_instances.json").read_text(encoding="utf-8"))[
        "instances"
    ]
    for row in instances.values():
        if row.get("name"):
            names.add(row["name"])
    for row in registry_rows:
        names.update(split_operator_field(row.operator))
    return {name for name in names if name}


def load_feedback_terms(known_operator_names: set[str]) -> dict[str, set[str]]:
    out: dict[str, set[str]] = {}
    for issue_path in sorted((ROOT / "feedback").glob("*/*/issue.json")):
        try:
            issue = json.loads(issue_path.read_text(encoding="utf-8"))
        except Exception:
            continue
        terms: set[str] = set()
        room = issue.get("room", {})
        for op in room.get("operators", []):
            if isinstance(op, str) and op:
                terms.add(op)
        note = issue.get("note", "")
        occupied_spans: list[tuple[int, int]] = []
        for name in sorted(known_operator_names, key=len, reverse=True):
            if name in note:
                start = note.find(name)
                end = start + len(name)
                if any(not (end <= a or start >= b) for a, b in occupied_spans):
                    continue
                occupied_spans.append((start, end))
                terms.add(name)
        if terms:
            out[str(issue_path.parent.relative_to(ROOT))] = terms
    return out


def system_terms_from_sources(
    system_ops: dict[str, set[str]], feedback_terms: dict[str, set[str]]
) -> set[str]:
    terms: set[str] = set()
    for ops in system_ops.values():
        terms.update(ops)
    for vals in feedback_terms.values():
        terms.update(vals)
    return {term for term in terms if term}


def tags_for(row: RegistryRow) -> list[str]:
    text = row.text
    tags = [name for name, terms in TAG_RULES if any(term in text for term in terms)]
    tags.extend(
        name
        for name, patterns in REGEX_TAG_RULES
        if any(pattern.search(text) for pattern in patterns)
    )
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


def split_operator_field(value: str) -> set[str]:
    return {part.strip() for part in re.split(r"[；;、,]", value) if part.strip()}


def facility_mentions(row: RegistryRow) -> dict[str, list[str]]:
    source = row.facility
    targets = sorted(
        {
            facility
            for cn, facility in FACILITY_MAP.items()
            if facility in RUNTIME_FACILITIES and cn in row.text and facility != source
        }
    )
    return {"source": source, "targets": targets}


def row_to_json(
    row: RegistryRow,
    tags: list[str],
    risk_terms: list[str],
    actionable_risk_terms: list[str],
    skill_status: str,
    instance_status_value: str,
    system_related: bool,
    feedback_related: bool,
    priority_score: int,
) -> dict[str, Any]:
    return {
        "index": row.index,
        "skill_name": row.skill_name,
        "facility_cn": row.facility_cn,
        "facility": row.facility,
        "product": row.product,
        "operator": row.operator,
        "operators": sorted(split_operator_field(row.operator)),
        "elite": row.elite,
        "efficiency": row.efficiency,
        "text": row.text,
        "tags": tags,
        "risk_terms": risk_terms,
        "actionable_risk_terms": actionable_risk_terms,
        "skill_table_status": skill_status,
        "instance_status": instance_status_value,
        "facility_mentions": facility_mentions(row),
        "system_related": system_related,
        "feedback_related": feedback_related,
        "priority_score": priority_score,
    }


def priority_score_for(
    row: RegistryRow,
    tags: list[str],
    actionable_risks: list[str],
    skill_status: str,
    instance_status_value: str,
    feedback_related: bool,
) -> int:
    score = 0
    if row.facility in RUNTIME_FACILITIES:
        score += 2
    if row.facility == "manufacture" or "manufacture" in facility_mentions(row)["targets"]:
        score += 3
    if skill_status in {"not_found_by_name", "empty_atoms"}:
        score += 2
    if instance_status_value == "not_bound":
        score += 1
    if actionable_risks:
        score += min(4, len(actionable_risks))
    if {"cross_room_synergy", "global_bonus", "resource_token", "pair_synergy", "same_room_synergy"} & set(tags):
        score += 3
    if feedback_related:
        score += 4
    return score


def analyze_rows(rows: list[RegistryRow]) -> dict[str, Any]:
    skills_by_key = load_skill_table()
    instance_pairs = load_instance_pairs()
    system_ops = load_system_operators()
    feedback_terms = load_feedback_terms(load_known_operator_names(rows))
    system_terms = system_terms_from_sources(system_ops, feedback_terms)

    row_tags = {row.index: tags_for(row) for row in rows}
    row_risks = {row.index: risk_terms_for(row) for row in rows}
    row_actionable_risks = {row.index: actionable_risk_terms_for(row) for row in rows}
    skill_status = {row.index: skill_table_status(row, skills_by_key) for row in rows}
    inst_status = {row.index: instance_status(row, instance_pairs) for row in rows}
    system_related = {
        row.index: any(term in row.text or term in row.operator or term in row.skill_name for term in system_terms)
        for row in rows
    }
    feedback_related = {
        row.index: any(
            term in row.text or term in row.operator or term in row.skill_name
            for terms in feedback_terms.values()
            for term in terms
        )
        for row in rows
    }
    priority_scores = {
        row.index: priority_score_for(
            row,
            row_tags[row.index],
            row_actionable_risks[row.index],
            skill_status[row.index],
            inst_status[row.index],
            feedback_related[row.index],
        )
        for row in rows
    }

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
            or system_related[row.index]
        )
    ]
    feedback_candidates = [row for row in runtime_rows if feedback_related[row.index]]

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
    priority_rows = sorted(
        [row for row in runtime_rows if priority_scores[row.index] >= 10],
        key=lambda row: (-priority_scores[row.index], row.facility_cn, row.operator, row.skill_name),
    )

    system_hits: list[dict[str, Any]] = []
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
            {
                "system_id": system_id,
                "operators": sorted(ops),
                "registry_rows": len(hits),
                "risk_rows": risky_count,
                "facilities": sorted({row.facility_cn for row in hits}),
            }
        )

    row_json = [
        row_to_json(
            row,
            row_tags[row.index],
            row_risks[row.index],
            row_actionable_risks[row.index],
            skill_status[row.index],
            inst_status[row.index],
            system_related[row.index],
            feedback_related[row.index],
            priority_scores[row.index],
        )
        for row in rows
    ]

    return {
        "summary": {
            "registry_rows": len(rows),
            "runtime_scope_rows": len(runtime_rows),
            "manufacture_rows": len(manu_rows),
            "manufacture_risk_rows": len(manu_risky),
            "cross_facility_global_candidate_rows": len(cross_rows),
            "high_risk_runtime_model_gap_rows": len(high_risk_gaps),
            "priority_manual_audit_rows": len(priority_rows),
        },
        "counts": {
            "facility": dict(facility_counts.most_common()),
            "tag": dict(tag_counts.most_common()),
            "risk_term": dict(risk_counts.most_common()),
            "skill_table_status": dict(skill_counts.most_common()),
            "instance_status": dict(inst_counts.most_common()),
        },
        "sources": {
            "system_terms": sorted(system_terms),
            "system_terms_source": "data/base_systems.json + feedback/*/*/issue.json",
            "feedback_terms": {key: sorted(value) for key, value in feedback_terms.items()},
        },
        "rows": row_json,
        "lists": {
            "manufacture_risk": [row.index for row in manu_risky],
            "cross_facility_global": [row.index for row in cross_rows],
            "system_candidates": [row.index for row in system_candidates],
            "feedback_candidates": [row.index for row in feedback_candidates],
            "high_risk_model_gaps": [row.index for row in high_risk_gaps],
            "priority_manual_audit": [row.index for row in priority_rows],
        },
        "system_overlap": system_hits,
        "_objects": {
            "rows_by_index": {row.index: row for row in rows},
            "manufacture_risk": manu_risky,
            "cross_facility_global": cross_rows,
            "system_candidates": system_candidates,
            "feedback_candidates": feedback_candidates,
            "high_risk_model_gaps": high_risk_gaps,
            "priority_manual_audit": priority_rows,
        },
    }


def build_report(analysis: dict[str, Any]) -> str:
    def j(row: RegistryRow) -> dict[str, Any]:
        return next(item for item in analysis["rows"] if item["index"] == row.index)

    lines: list[str] = []
    append = lines.append
    append("# MECHANICS_REGISTRY Audit")
    append("")
    append("> Generated by `python3 scripts/audit_mechanics_registry.py`. This is a read-only report: rows marked as candidates still require manual confirmation before entering runtime rules.")
    append("")
    append("## Summary")
    append("")
    summary = analysis["summary"]
    append(f"- Registry rows: {summary['registry_rows']}")
    append(f"- Runtime-scope rows: {summary['runtime_scope_rows']} (`trade`, `manufacture`, `power`, `control`, `dorm`, `office`)")
    append(f"- Manufacture rows: {summary['manufacture_rows']}")
    append(f"- Manufacture risk rows: {summary['manufacture_risk_rows']}")
    append(f"- Cross-facility / global candidate rows: {summary['cross_facility_global_candidate_rows']}")
    append(f"- High-risk runtime model-gap rows: {summary['high_risk_runtime_model_gap_rows']}")
    append(f"- Top-priority manual audit rows: {summary['priority_manual_audit_rows']}")
    append("")
    append("Important caveat: `skill_table_status` is a coarse triage signal, not coverage proof. It matches by runtime facility and Chinese skill name, so name drift, buff-id-only modeling, reused skill names, and multi-operator registry rows can all produce false positives.")
    append("")
    append("Machine-readable full output: `python3 scripts/audit_mechanics_registry.py --json-output target/generated/mechanics_registry_audit_full.json`.")
    append("")
    append("## Facility Counts")
    append("")
    lines.extend(bullet_counts(Counter(analysis["counts"]["facility"])))
    append("")
    append("## Runtime Coverage Snapshot")
    append("")
    append("This is a coarse name/facility audit. It is useful for triage, not proof of semantic coverage.")
    append("")
    append("Skill-table status:")
    lines.extend(bullet_counts(Counter(analysis["counts"]["skill_table_status"])))
    append("")
    append("Operator-instance status:")
    lines.extend(bullet_counts(Counter(analysis["counts"]["instance_status"])))
    append("")
    append("## Tag Taxonomy Counts")
    append("")
    lines.extend(bullet_counts(Counter(analysis["counts"]["tag"])))
    append("")
    append("## High-Risk Keyword Counts")
    append("")
    lines.extend(bullet_counts(Counter(analysis["counts"]["risk_term"])))
    append("")
    append("## Top Priority Manual Audit")
    append("")
    append("Priority favors runtime-scope rows that affect manufacture, carry coupling/global/high-risk tags, have coarse model gaps, and mention recent feedback terms.")
    append("")
    lines.extend(
        table(
            ["#", "score", "facility", "operator", "skill", "status", "tags", "targets", "text"],
            [
                [
                    row.index,
                    j(row)["priority_score"],
                    row.facility_cn,
                    row.operator,
                    row.skill_name,
                    f"{j(row)['skill_table_status']} / {j(row)['instance_status']}",
                    ", ".join(j(row)["tags"]) or "-",
                    ", ".join(j(row)["facility_mentions"]["targets"]) or "-",
                    truncate(row.text),
                ]
                for row in analysis["_objects"]["priority_manual_audit"][:30]
            ],
        )
    )
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
                    ", ".join(j(row)["tags"]) or "-",
                    ", ".join(j(row)["actionable_risk_terms"]) or "-",
                    j(row)["skill_table_status"],
                    truncate(row.text),
                ]
                for row in analysis["_objects"]["manufacture_risk"][:40]
            ],
        )
    )
    append("")
    append("## Cross-Facility / Global Candidate Rows")
    append("")
    lines.extend(
        table(
            ["#", "source", "targets", "operator", "skill", "tags", "risk terms", "text"],
            [
                [
                    row.index,
                    row.facility_cn,
                    ", ".join(j(row)["facility_mentions"]["targets"]) or "-",
                    row.operator,
                    row.skill_name,
                    ", ".join(j(row)["tags"]) or "-",
                    ", ".join(j(row)["actionable_risk_terms"]) or "-",
                    truncate(row.text),
                ]
                for row in analysis["_objects"]["cross_facility_global"][:60]
            ],
        )
    )
    append("")
    append("## System Candidate Discovery")
    append("")
    append("Rows below are candidates for human review only. They are selected by coupling tags, high-risk terms, and system/feedback terms loaded from data, not a hand-maintained operator list.")
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
                    ", ".join(j(row)["tags"]) or "-",
                    ", ".join(j(row)["actionable_risk_terms"]) or "-",
                    truncate(row.text),
                ]
                for row in analysis["_objects"]["system_candidates"][:60]
            ],
        )
    )
    append("")
    append("## Existing base_systems Overlap")
    append("")
    lines.extend(
        table(
            ["system_id", "operators", "registry rows", "risk rows", "facilities"],
            [
                [
                    row["system_id"],
                    ", ".join(row["operators"]),
                    row["registry_rows"],
                    row["risk_rows"],
                    ", ".join(row["facilities"]),
                ]
                for row in analysis["system_overlap"]
            ],
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
                    ", ".join(j(row)["tags"]) or "-",
                    ", ".join(j(row)["actionable_risk_terms"]) or "-",
                    truncate(row.text),
                ]
                for row in analysis["_objects"]["feedback_candidates"][:60]
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
                    j(row)["skill_table_status"],
                    j(row)["instance_status"],
                    ", ".join(j(row)["tags"]) or "-",
                    ", ".join(j(row)["actionable_risk_terms"]) or "-",
                    truncate(row.text),
                ]
                for row in analysis["_objects"]["high_risk_model_gaps"][:80]
            ],
        )
    )
    append("")
    append("## Test Suggestions")
    append("")
    append("- Draft seed: `data/feedback_regression_seeds/purestream_weedy_windflit_gold_automation.json` captures preferred pattern `清流 + 温蒂 + 冬时`, linked producer `承曦格雷伊`, and max regret tolerance.")
    append("- Add a trace expectation that explains whether a system candidate was selected by near-optimal preference or rejected by raw-score gap/operator conflict.")
    append("- Use the high-risk model-gap table to choose manual audits for cross-room/global tokens before adding any runtime rule.")
    append("")
    return "\n".join(lines) + "\n"


def json_ready(analysis: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in analysis.items() if key != "_objects"}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, default=DATA / "MECHANICS_REGISTRY.csv")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--json-output", type=Path, default=DEFAULT_JSON_OUTPUT)
    args = parser.parse_args()

    rows = load_registry(args.registry)
    analysis = analyze_rows(rows)
    report = build_report(analysis)
    output = args.output if args.output.is_absolute() else ROOT / args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(report, encoding="utf-8")
    json_output = args.json_output if args.json_output.is_absolute() else ROOT / args.json_output
    json_output.parent.mkdir(parents=True, exist_ok=True)
    json_output.write_text(
        json.dumps(json_ready(analysis), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {output.relative_to(ROOT)}")
    print(f"wrote {json_output.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
