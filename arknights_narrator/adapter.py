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
        return cls(
            provider=os.getenv("TRAINING_ADVICE_LLM_PROVIDER", "openai"),
            base_url=os.getenv("TRAINING_ADVICE_LLM_BASE_URL", "https://api.openai.com/v1"),
            model=os.getenv("TRAINING_ADVICE_LLM_MODEL", ""),
            api_key=os.getenv("TRAINING_ADVICE_LLM_API_KEY", ""),
            request_timeout_ms=int(os.getenv("TRAINING_ADVICE_LLM_REQUEST_TIMEOUT_MS", "12000")),
            task_deadline_ms=int(os.getenv("TRAINING_ADVICE_NARRATIVE_TASK_DEADLINE_MS", "20000")),
            max_output_tokens=int(os.getenv("TRAINING_ADVICE_LLM_MAX_OUTPUT_TOKENS", "2200")),
            max_chars=int(os.getenv("TRAINING_ADVICE_NARRATIVE_MAX_CHARS", "6000")),
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
            validated = validate_and_replace(raw, value, self.config.max_chars)
            return NarratorResult("ready", validated)
        except Exception as first_error:  # noqa: BLE001 - fallback is the public failure contract.
            if time.monotonic() >= deadline:
                return NarratorResult("fallback", render_fallback(value, "task_deadline"), "task_deadline")
            try:
                raw = self._call_repair(value, deadline, str(first_error))
                validated = validate_and_replace(raw, value, self.config.max_chars)
                return NarratorResult("ready", validated)
            except Exception:  # noqa: BLE001
                return NarratorResult("fallback", render_fallback(value, "invalid_llm_output"), "invalid_llm_output")

    def _call(self, value: NarrativeInput, deadline: float) -> str:
        payload = input_json(value)
        for attempt in range(2):
            timeout = min(self.config.request_timeout_ms / 1000, max(0.1, deadline - time.monotonic()))
            try:
                return self.client.generate(_system_prompt(), payload, timeout=timeout, max_tokens=self.config.max_output_tokens)  # type: ignore[union-attr]
            except Exception as exc:  # noqa: BLE001 - provider errors are classified below.
                if attempt == 0 and _retryable(exc) and time.monotonic() < deadline:
                    time.sleep(min(0.5, max(0.0, deadline - time.monotonic())))
                    continue
                raise

    def _call_repair(self, value: NarrativeInput, deadline: float, error: str) -> str:
        timeout = min(self.config.request_timeout_ms / 1000, max(0.1, deadline - time.monotonic()))
        payload = input_json(value) + "\n校验错误：" + error
        return self.client.generate(_system_prompt() + "\n请只修复 JSON 协议错误。", payload, timeout=timeout, max_tokens=self.config.max_output_tokens)  # type: ignore[union-attr]


def validate_and_replace(raw: str | dict[str, Any], value: NarrativeInput, max_chars: int = 6_000) -> dict[str, Any]:
    data = json.loads(raw) if isinstance(raw, str) else raw
    if not isinstance(data, dict) or data.get("schema_version") != 1:
        raise ValueError("narrative schema_version must be 1")
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
        rendered = dict(section)
        for field in ("paragraphs", "items"):
            if field not in section:
                continue
            if not isinstance(section[field], list) or not all(isinstance(item, str) for item in section[field]):
                raise ValueError(f"{field} must be a string list")
            values = []
            for text in section[field]:
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


def _system_prompt() -> str:
    path = Path(__file__).with_name("system_prompt.md")
    return path.read_text(encoding="utf-8")


def _retryable(error: Exception) -> bool:
    status_code = getattr(error, "status_code", None)
    return status_code in {429, 502, 503, 504} or isinstance(
        error, (TimeoutError, ConnectionError, OSError)
    )
