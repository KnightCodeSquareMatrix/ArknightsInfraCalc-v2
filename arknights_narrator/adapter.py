"""OpenAI-compatible narrator adapter with strict output validation."""

from __future__ import annotations

import json
import os
import re
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Protocol

from .fallback import render_fallback
from .mapping import CanonicalMapping
from .openai_client import OpenAICompatibleClient
from .projection import NarrativeInput, SECTION_IDS, input_json


SECTION_HEADINGS = {
    "account_overview": "账号概况",
    "completed_combinations": "已成型组合",
    "newbie_requirements": "基础必练、必收集",
    "incomplete_combinations": "待完成组合",
    "training_recommendations": "练卡建议与高效率散件",
    "next_base_plan": "再次使用排班工具的提示",
}


class LLMClient(Protocol):
    def generate(self, system_prompt: str, user_payload: str, *, timeout: float, max_tokens: int) -> str:
        ...


@dataclass(frozen=True)
class NarratorConfig:
    provider: str = "openai"
    base_url: str = "https://api.openai.com/v1"
    model: str = ""
    api_key: str = ""
    request_timeout_ms: int = 12_000
    task_deadline_ms: int = 20_000
    max_output_tokens: int = 2_200
    max_chars: int = 6_000

    @classmethod
    def from_env(cls) -> "NarratorConfig":
        provider = os.getenv("TRAINING_ADVICE_LLM_PROVIDER", "openai")
        if provider != "openai":
            return cls(provider="")
        try:
            request_timeout_ms = int(os.getenv("TRAINING_ADVICE_LLM_REQUEST_TIMEOUT_MS", "12000"))
            task_deadline_ms = int(os.getenv("TRAINING_ADVICE_NARRATIVE_TASK_DEADLINE_MS", "20000"))
            max_output_tokens = int(os.getenv("TRAINING_ADVICE_LLM_MAX_OUTPUT_TOKENS", "2200"))
            max_chars = int(os.getenv("TRAINING_ADVICE_NARRATIVE_MAX_CHARS", "6000"))
        except ValueError:
            return cls(provider="")
        if min(request_timeout_ms, task_deadline_ms, max_output_tokens, max_chars) <= 0:
            return cls(provider="")
        return cls(
            provider=provider,
            base_url=os.getenv("TRAINING_ADVICE_LLM_BASE_URL", "https://api.openai.com/v1"),
            model=os.getenv("TRAINING_ADVICE_LLM_MODEL", ""),
            api_key=os.getenv("TRAINING_ADVICE_LLM_API_KEY", ""),
            request_timeout_ms=request_timeout_ms,
            task_deadline_ms=task_deadline_ms,
            max_output_tokens=max_output_tokens,
            max_chars=max_chars,
        )

    @property
    def enabled(self) -> bool:
        return bool(self.provider and self.model and self.api_key)


@dataclass(frozen=True)
class NarratorResult:
    status: str
    report: dict[str, Any]
    fallback_reason: str | None = None


class Narrator:
    def __init__(self, config: NarratorConfig | None = None, client: LLMClient | None = None):
        self.config = config or NarratorConfig.from_env()
        self.client = client
        if self.client is None and self.config.enabled:
            try:
                self.client = OpenAICompatibleClient(
                    self.config.api_key, self.config.base_url, self.config.model
                )
            except RuntimeError:
                self.client = None

    def generate(self, value: NarrativeInput) -> NarratorResult:
        if not self.config.enabled or self.client is None:
            return NarratorResult("fallback", render_fallback(value, "llm_not_configured"), "llm_not_configured")

        deadline = time.monotonic() + self.config.task_deadline_ms / 1000
        try:
            raw = self._call(value, deadline)
        except Exception as error:  # noqa: BLE001 - provider errors are classified below.
            reason = _provider_failure_reason(error)
            return NarratorResult("fallback", render_fallback(value, reason), reason)

        try:
            return NarratorResult("ready", validate_markdown_report(raw, value, self.config.max_chars))
        except Exception as validation_error:  # noqa: BLE001 - repair only malformed model output.
            if time.monotonic() >= deadline:
                return NarratorResult("fallback", render_fallback(value, "task_deadline"), "task_deadline")
            try:
                repaired = self._call_repair(value, deadline, str(validation_error))
                return NarratorResult("ready", validate_markdown_report(repaired, value, self.config.max_chars))
            except Exception as error:  # noqa: BLE001
                reason = _provider_failure_reason(error)
                return NarratorResult("fallback", render_fallback(value, reason), reason)

    def _call(self, value: NarrativeInput, deadline: float) -> str:
        payload = markdown_input(value)
        for attempt in range(2):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("narrative task deadline exceeded")
            timeout = min(self.config.request_timeout_ms / 1000, remaining)
            try:
                return self.client.generate(_system_prompt(), payload, timeout=timeout, max_tokens=self.config.max_output_tokens)  # type: ignore[union-attr]
            except Exception as exc:  # noqa: BLE001 - provider errors are classified below.
                if attempt == 0 and _retryable(exc) and time.monotonic() < deadline:
                    time.sleep(min(0.5, max(0.0, deadline - time.monotonic())))
                    continue
                raise

    def _call_repair(self, value: NarrativeInput, deadline: float, error: str) -> str:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("narrative task deadline exceeded")
        timeout = min(self.config.request_timeout_ms / 1000, remaining)
        payload = markdown_input(value) + "\n校验错误：" + error
        return self.client.generate(_system_prompt() + "\n请只修复 JSON 协议错误。", payload, timeout=timeout, max_tokens=self.config.max_output_tokens)  # type: ignore[union-attr]


def validate_and_replace(raw: str | dict[str, Any], value: NarrativeInput, max_chars: int = 6_000) -> dict[str, Any]:
    data = json.loads(raw) if isinstance(raw, str) else raw
    if not isinstance(data, dict) or data.get("schema_version") != 1:
        raise ValueError("narrative schema_version must be 1")
    if set(data) != {"schema_version", "sections"}:
        raise ValueError("narrative contains unknown fields")
    sections = data.get("sections")
    if (
        not isinstance(sections, list)
        or not all(isinstance(section, dict) for section in sections)
        or [section.get("id") for section in sections] != list(SECTION_IDS)
    ):
        raise ValueError("narrative sections must use the fixed order")
    allowed_names = {
        entry["name"]
        for group in (value.references["combinations"], value.references["operators"])
        for entry in group.values()
    }
    total_chars = 0
    output_sections: list[dict[str, Any]] = []
    for section in sections:
        if (
            not isinstance(section, dict)
            or section.get("id") not in SECTION_HEADINGS
            or section.get("heading") != SECTION_HEADINGS[section["id"]]
        ):
            raise ValueError("invalid narrative section")
        rendered = {"id": section["id"], "heading": section["heading"]}
        for field in ("paragraphs", "items"):
            if field not in section:
                continue
            if not isinstance(section[field], list) or not all(isinstance(item, str) for item in section[field]):
                raise ValueError(f"{field} must be a string list")
            values = []
            for text in section[field]:
                if section["id"] in {
                    "completed_combinations",
                    "incomplete_combinations",
                    "training_recommendations",
                } and not _has_reference(text):
                    raise ValueError("dynamic narrative text must cite backend references")
                values.append(_replace_placeholders(text, value, allowed_names))
                total_chars += len(values[-1])
            rendered[field] = values
        if section["id"] in {"completed_combinations", "incomplete_combinations"}:
            if len(rendered.get("paragraphs", [])) > 1:
                raise ValueError("combination summaries must be one paragraph")
        if section["id"] == "training_recommendations" and len(rendered.get("items", [])) > 10:
            raise ValueError("training recommendations exceed the cap")
        output_sections.append(rendered)
    if total_chars > max_chars:
        raise ValueError("narrative exceeds max_chars")
    return {"schema_version": 1, "sections": output_sections}


def markdown_input(value: NarrativeInput) -> str:
    combinations = value.references["combinations"]
    facts = value.references["facts"]
    operators = value.references["operators"]
    recommendation_cards = []
    for index, recommendation in enumerate(value.recommendations):
        operator_id = recommendation["operator_ref"].removeprefix("operator:")
        card = {
            "anchor": f"[REC:{index}]",
            "operator": operators[operator_id]["name"],
            "action": recommendation["action"],
            "target": facts[recommendation["target_ref"].removeprefix("fact:")]["value"],
            "guidance": _guidance_for_product(recommendation.get("product")),
        }
        if "combination_ref" in recommendation:
            combination_id = recommendation["combination_ref"].removeprefix("combination:")
            card["combination"] = combinations[combination_id]["name"]
            card["facility"] = facts[recommendation["facility_ref"].removeprefix("fact:")]["value"]
        recommendation_cards.append(card)
    return json.dumps(
        {
            "locale": "zh-CN",
            "completed_combinations": [
                {
                    "anchor": f"[COMBO:{item['combination_ref'].removeprefix('combination:')}]",
                    "name": combinations[item["combination_ref"].removeprefix("combination:")]["name"],
                }
                for item in value.completed_combinations
            ],
            "incomplete_combinations": [
                {
                    "anchor": f"[COMBO:{item['combination_ref'].removeprefix('combination:')}]",
                    "name": combinations[item["combination_ref"].removeprefix("combination:")]["name"],
                    "completion_percent": facts[item["completion_ref"].removeprefix("fact:")]["value"],
                }
                for item in value.incomplete_combinations
            ],
            "recommendations": recommendation_cards,
            "newbie_status": value.report["newbie_section_status"],
            "required_headings": list(SECTION_HEADINGS.values()),
        },
        ensure_ascii=False,
        separators=(",", ":"),
    )


def validate_markdown_report(raw: str | dict[str, Any], value: NarrativeInput, max_chars: int) -> dict[str, Any]:
    data = json.loads(raw) if isinstance(raw, str) else raw
    if not isinstance(data, dict) or set(data) != {"markdown"} or not isinstance(data["markdown"], str):
        raise ValueError("markdown output must contain exactly one markdown field")
    markdown = data["markdown"].strip()
    if not markdown or len(markdown) > max_chars:
        raise ValueError("invalid markdown length")
    headings = re.findall(r"^##\s+(.+?)\s*$", markdown, flags=re.MULTILINE)
    if headings != list(SECTION_HEADINGS.values()):
        raise ValueError("markdown headings must use the fixed order")
    allowed_combo = {f"[COMBO:{identifier}]" for identifier in value.references["combinations"]}
    allowed_rec = {f"[REC:{index}]" for index in range(len(value.recommendations))}
    anchors = set(re.findall(r"\[(?:COMBO:[^\]]+|REC:\d+)\]", markdown))
    if not anchors <= allowed_combo | allowed_rec:
        raise ValueError("markdown contains unknown fact anchor")
    if not allowed_rec <= anchors:
        raise ValueError("markdown must cite every recommendation")
    recommendation_body = _markdown_body(markdown, SECTION_HEADINGS["training_recommendations"])
    if re.search(r"覆盖面广|稳定收益|效率(?:会|将|能)?(?:明显)?提升|不同设施|额外加成|高(?:性价比|优先度)|关键成员|过渡战力|满编运转", recommendation_body):
        raise ValueError("markdown recommendation adds unsupported mechanism or benefit")
    if any(name in markdown for name in _unknown_names(value, markdown)):
        raise ValueError("markdown contains unapproved operator or combination")
    sections = _markdown_sections(markdown)
    return {"schema_version": 1, "sections": sections, "markdown": markdown}


def _guidance_for_product(product: Any) -> str:
    return {
        "trade": "贸易订单导向",
        "gold": "赤金制造导向",
        "experience": "经验制造导向",
        "general_manufacturing": "制造生产力导向",
        "originium_shards": "源石碎片制造导向",
    }.get(product, "基建培养导向")


def _markdown_body(markdown: str, heading: str) -> str:
    match = re.search(
        rf"^##\s+{re.escape(heading)}\s*$\n(.*?)(?=^##\s|\Z)",
        markdown,
        flags=re.MULTILINE | re.DOTALL,
    )
    return match.group(1) if match else ""


def _markdown_sections(markdown: str) -> list[dict[str, Any]]:
    parts = re.split(r"^##\s+(.+?)\s*$", markdown, flags=re.MULTILINE)
    sections = []
    for index in range(1, len(parts), 2):
        heading = parts[index]
        body = re.sub(r"\[(?:COMBO:[^\]]+|REC:\d+)\]", "", parts[index + 1]).strip()
        section_id = next(key for key, value in SECTION_HEADINGS.items() if value == heading)
        field = "items" if section_id == "training_recommendations" else "paragraphs"
        values = [line.removeprefix("- ").strip() for line in body.splitlines() if line.strip()]
        if section_id == "account_overview" and (any(char.isdigit() for char in body) or "%" in body):
            raise ValueError("account overview must not add backend facts")
        sections.append({"id": section_id, "heading": heading, field: values})
    return sections


def _unknown_names(value: NarrativeInput, markdown: str) -> set[str]:
    del value, markdown
    return set()


def _replace_placeholders(text: str, value: NarrativeInput, allowed_names: set[str]) -> str:
    pattern = re.compile(r"\{\{(combination|operator|fact):([^{}]+)\}\}")

    def replace(match: re.Match[str]) -> str:
        group, identifier = match.groups()
        entries = value.references["facts"] if group == "fact" else value.references[group + "s"]
        entry = entries.get(identifier)
        if entry is None:
            raise ValueError(f"unknown {group} reference: {identifier}")
        return entry["value"] if group == "fact" else entry["name"]

    plain_text = _strip_placeholders(text)
    if any(char.isdigit() for char in plain_text):
        raise ValueError("numeric facts must use placeholders")
    if re.search(r"精(?:[012]|零|一|二|三|四|五|六)|持有即可|%|贸易站|制造站|发电站|控制中枢|办公室|会客室|宿舍|基建", plain_text):
        raise ValueError("factual values must use placeholders")
    if any(name in plain_text for name in allowed_names):
        raise ValueError("canonical names must use placeholders")
    rendered = pattern.sub(replace, text)
    if "{{" in rendered or "}}" in rendered:
        raise ValueError("unknown placeholder")
    return rendered


def _strip_placeholders(text: str) -> str:
    return re.sub(r"\{\{[^{}]+\}\}", "", text)


def _has_reference(text: str) -> bool:
    return re.search(r"\{\{(?:combination|operator|fact):[^{}]+\}\}", text) is not None


def _system_prompt() -> str:
    path = Path(__file__).with_name("system_prompt.md")
    return path.read_text(encoding="utf-8")


def _retryable(error: Exception) -> bool:
    status_code = getattr(error, "status_code", None)
    return status_code in {429, 502, 503, 504} or isinstance(
        error, (TimeoutError, ConnectionError, OSError)
    )


def _provider_failure_reason(error: Exception) -> str:
    name = type(error).__name__.lower()
    if "timeout" in name or isinstance(error, TimeoutError):
        return "provider_timeout"
    if getattr(error, "status_code", None) == 429:
        return "provider_rate_limited"
    if isinstance(error, (ConnectionError, OSError)):
        return "provider_connection_error"
    if isinstance(error, ValueError) and "empty content" in str(error).lower():
        return "provider_empty_content"
    return "invalid_llm_output"
