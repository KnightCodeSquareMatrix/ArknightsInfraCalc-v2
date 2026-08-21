"""Deterministic fallback renderer."""

from __future__ import annotations

from typing import Any


def render_fallback(value: Any, reason: str = "llm_unavailable") -> dict[str, Any]:
    completed = value.completed_combinations
    incomplete = value.incomplete_combinations
    completed_text = _combination_sentence(completed, "已经达到成型可用状态")
    incomplete_text = _incomplete_sentence(incomplete)
    recommendations = [
        _recommendation_text(item, value.references)
        for item in value.recommendations[:4]
    ]
    sections = [
        {"id": "account_overview", "heading": "账号概况", "paragraphs": [_overview(value)]},
        {"id": "completed_combinations", "heading": "已成型组合", "paragraphs": [completed_text] if completed_text else []},
        {"id": "newbie_requirements", "heading": "基础必练、必收集", "paragraphs": [_newbie_text(value)]},
        {"id": "incomplete_combinations", "heading": "待完成组合", "paragraphs": [incomplete_text] if incomplete_text else []},
        {"id": "training_recommendations", "heading": "练卡建议与高效率散件", "items": recommendations},
        {"id": "next_base_plan", "heading": "再次使用排班工具的提示", "paragraphs": ["练成关键角色后，可以再次使用排班工具确认实际进驻方案。"]},
    ]
    return {"schema_version": 1, "status": "fallback", "fallback_reason": reason, "sections": sections}


def _overview(value: Any) -> str:
    if value.completed_combinations and not value.incomplete_combinations:
        return "当前已有多组组合达到成型可用状态，暂未发现需要优先补齐的组合缺口。"
    if value.completed_combinations:
        return "当前已有部分组合达到成型可用状态，接下来可优先处理完成度较高的未完成组合。"
    return "当前组合尚未形成稳定的成型形态，建议先按下方确定性练卡建议逐项补齐。"


def _newbie_text(value: Any) -> str:
    status = value.report.get("newbie_section_status")
    if status == "complete":
        return "基础必练、必收集目标已经完成。"
    if status == "skipped_by_efficiency":
        return "当前排班效率达到跳过基础章节的条件；未完成事实仍保留在结构化详情中。"
    count = len(value.report.get("incomplete_newbie", []))
    return f"基础必练、必收集仍有 {count} 项未完成，请结合下方行动建议安排培养。"


def _combination_sentence(items: list[dict[str, Any]], suffix: str) -> str:
    names = [item["name"] for item in items]
    if not names:
        return ""
    return f"{_join(names)}等组合{suffix}。"


def _incomplete_sentence(items: list[dict[str, Any]]) -> str:
    if not items:
        return ""
    values = [f"{item['name']}（{item['completion_percent']}%）" for item in items]
    return f"{_join(values)}等组合尚未完成。"


def _recommendation_text(item: dict[str, Any], references: dict[str, dict[str, dict[str, str]]]) -> str:
    operator = item["operator"]
    target = item.get("target", {})
    target_text = _target_text(target)
    action = "收集并培养" if item.get("action") == "acquire" else "培养"
    combination = item.get("combination_name")
    suffix = f"，用于完成{combination}" if combination else "，用于提升基建效率"
    return f"建议{action}{operator}至{target_text}{suffix}。"


def _target_text(target: dict[str, Any]) -> str:
    kind = target.get("kind")
    if kind == "explicit":
        return {0: "精零", 1: "精一", 2: "精二"}.get(target.get("elite"), "目标练度")
    if kind == "no_requirement":
        return "持有即可"
    return "目标练度"


def _join(values: list[str]) -> str:
    if len(values) <= 1:
        return values[0] if values else ""
    if len(values) == 2:
        return "、".join(values)
    return "、".join(values[:-1])
