"""Project Rust player advice into a constrained LLM input."""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

from .mapping import CanonicalMapping, MappingError


SECTION_IDS = (
    "account_overview",
    "completed_combinations",
    "newbie_requirements",
    "incomplete_combinations",
    "training_recommendations",
    "next_base_plan",
)


@dataclass(frozen=True)
class NarrativeInput:
    schema_version: int
    locale: str
    mapping_version: str
    mapping_hash: str
    report: dict[str, Any]
    completed_combinations: list[dict[str, Any]]
    incomplete_combinations: list[dict[str, Any]]
    recommendations: list[dict[str, Any]]
    references: dict[str, dict[str, str]]

    def as_dict(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "locale": self.locale,
            "mapping_version": self.mapping_version,
            "mapping_hash": self.mapping_hash,
            "required_section_ids": list(SECTION_IDS),
            "report": self.report,
            "completed_combinations": self.completed_combinations,
            "incomplete_combinations": self.incomplete_combinations,
            "recommendations": self.recommendations,
            "references": self.references,
        }


def project_report(report: dict[str, Any], mapping: CanonicalMapping) -> NarrativeInput:
    if report.get("schema_version") != 2:
        raise ValueError("PlayerTrainingAdviceReport schema_version must be 2")
    combinations = report.get("combinations")
    if not isinstance(combinations, list):
        raise ValueError("report.combinations must be a list")
    recommendations = report.get("recommendations", [])
    if not isinstance(recommendations, list):
        raise ValueError("report.recommendations must be a list")
    recommendations = recommendations[:10]

    seen_ids: set[str] = set()
    normalized: list[tuple[int, dict[str, Any]]] = []
    for index, combination in enumerate(combinations):
        identifier = combination.get("id")
        name = combination.get("name")
        if not isinstance(identifier, str) or not isinstance(name, str) or identifier in seen_ids:
            raise ValueError("combination ids and names must be unique strings")
        seen_ids.add(identifier)
        if mapping.combination_name(identifier) != name:
            raise MappingError(f"mapping mismatch for combination {identifier}")
        normalized.append((index, combination))

    completed = [
        item
        for _, item in sorted(normalized, key=lambda pair: (-pair[1]["completion_percent"], pair[0]))
        if item.get("state") == "complete"
    ][:6]
    incomplete = [
        item
        for _, item in sorted(normalized, key=lambda pair: (-pair[1]["completion_percent"], pair[0]))
        if item.get("state") != "complete" and item.get("state") != "needs_review"
    ][:6]
    combinations_by_id = {item["id"]: item for _, item in normalized}
    references: dict[str, dict[str, Any]] = {"combinations": {}, "operators": {}, "facts": {}}

    # Only selected summaries and recommendation-linked combinations are visible to the model.
    selected = {item["id"]: item for item in [*completed, *incomplete]}
    for recommendation in recommendations:
        combination_id = recommendation.get("combination_id")
        if combination_id is not None:
            if combination_id not in combinations_by_id:
                raise ValueError(f"recommendation references unknown combination: {combination_id}")
            selected[combination_id] = combinations_by_id[combination_id]
    for identifier, combination in selected.items():
        references["combinations"][identifier] = {"name": combination["name"]}
        references["facts"][f"completion:{identifier}"] = {
            "value": f"{combination['completion_percent']}%"
        }

    newbie_items = report.get("incomplete_newbie", [])
    if not isinstance(newbie_items, list):
        raise ValueError("report.incomplete_newbie must be a list")
    references["facts"]["newbie_count"] = {"value": str(len(newbie_items))}
    references["facts"]["newbie_status"] = {
        "value": str(report.get("newbie_section_status", "shown"))
    }

    projected_recommendations: list[dict[str, Any]] = []
    for recommendation_index, recommendation in enumerate(recommendations):
        operator_id = _register_operator(recommendation.get("operator"), references["operators"], mapping)
        combination_id = recommendation.get("combination_id")
        if combination_id is not None and recommendation.get("combination_name") not in {
            None,
            references["combinations"][combination_id]["name"],
        }:
            raise MappingError(f"mapping mismatch for recommendation combination {combination_id}")
        target = recommendation.get("target")
        if not isinstance(target, dict):
            raise ValueError("recommendation target must be an object")
        references["facts"][f"target:{recommendation_index}"] = {
            "value": _target_text(target)
        }
        facilities = []
        if combination_id is not None:
            combination = combinations_by_id[combination_id]
            facilities = combination.get("facilities", [])
        references["facts"][f"facility:{recommendation_index}"] = {
            "value": _facility_text(facilities)
        }
        projected_recommendation = {
            "operator_ref": f"operator:{operator_id}",
            "action": recommendation.get("action"),
            "target_ref": f"fact:target:{recommendation_index}",
            "facility_ref": f"fact:facility:{recommendation_index}",
        }
        if combination_id is not None:
            projected_recommendation["combination_ref"] = f"combination:{combination_id}"
        if recommendation.get("product") is not None:
            projected_recommendation["product"] = recommendation["product"]
        if recommendation.get("acquisition") is not None:
            projected_recommendation["acquisition"] = recommendation["acquisition"]
        if recommendation.get("efficiency") is not None:
            projected_recommendation["efficiency"] = recommendation["efficiency"]
        projected_recommendations.append(projected_recommendation)

    return NarrativeInput(
        schema_version=1,
        locale="zh-CN",
        mapping_version=mapping.version,
        mapping_hash=mapping.content_hash,
        report={"newbie_section_status": report.get("newbie_section_status")},
        completed_combinations=[_summary(item) for item in completed],
        incomplete_combinations=[_summary(item) for item in incomplete],
        recommendations=projected_recommendations,
        references=references,
    )


def _register_operator(operator: Any, target: dict[str, dict[str, str]], mapping: CanonicalMapping) -> str:
    if not isinstance(operator, str) or not operator.strip():
        raise ValueError("operator names must be non-empty strings")
    identifier = mapping.operator_ids_by_name.get(operator)
    if identifier is None:
        raise MappingError(f"mapping mismatch for operator {operator}")
    target[identifier] = {"name": operator}
    return identifier


def _summary(combination: dict[str, Any]) -> dict[str, Any]:
    return {
        "combination_ref": f"combination:{combination['id']}",
        "completion_ref": f"fact:completion:{combination['id']}",
    }


def _target_text(target: dict[str, Any]) -> str:
    if target.get("kind") == "explicit":
        return {0: "精零", 1: "精一", 2: "精二"}.get(target.get("elite"), "目标练度")
    if target.get("kind") == "no_requirement":
        return "持有即可"
    return "目标练度"


def _facility_text(facilities: list[Any]) -> str:
    labels = {
        "trade_station": "贸易站",
        "manufacturing_station": "制造站",
        "power_station": "发电站",
        "control_center": "控制中枢",
        "office": "人力办公室",
        "meeting_room": "会客室",
        "dormitory": "宿舍",
        "base": "基建",
    }
    names = [labels.get(value, "基建") for value in facilities if isinstance(value, str)]
    return "、".join(names) if names else "基建"


def input_json(value: NarrativeInput) -> str:
    return json.dumps(value.as_dict(), ensure_ascii=False, sort_keys=True, separators=(",", ":"))
