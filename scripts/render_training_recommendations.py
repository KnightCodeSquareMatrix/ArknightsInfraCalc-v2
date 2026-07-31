#!/usr/bin/env python3
"""Render training recommendation rules (v2) as a deterministic Chinese review draft."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_INPUT = REPO_ROOT / "data" / "training_recommendations.json"
PRIORITIES = ("P0", "P1", "P2", "Info")
KINDS = ("system", "combo", "standalone", "soft_combo")
ROLES = ("core", "important", "hanger", "independent")


class RuleFormatError(ValueError):
    pass


def load_rules(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise RuleFormatError(f"无法读取规则文件 {path}: {error}") from error
    if not isinstance(value, dict):
        raise RuleFormatError("规则文件顶层必须是 JSON object")
    if value.get("version") != 2:
        raise RuleFormatError("version 必须是 2")
    if not isinstance(value.get("rules"), list):
        raise RuleFormatError("rules 必须是数组")
    return value


def is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def require_object(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise RuleFormatError(f"{context} 必须是 object")
    return value


def require_string(value: dict[str, Any], key: str, context: str) -> str:
    result = value.get(key)
    if not isinstance(result, str) or not result.strip():
        raise RuleFormatError(f"{context}.{key} 必须是非空字符串")
    return result.strip()


def format_target(target: Any, context: str) -> str:
    target = require_object(target, context)
    elite = target.get("elite")
    if not is_int(elite) or elite < 0:
        raise RuleFormatError(f"{context}.elite 必须是非负整数")
    level = target.get("level")
    if level is not None and (not is_int(level) or level < 1):
        raise RuleFormatError(f"{context}.level 必须是正整数")
    if elite == 0:
        body = f"E0/{level}级" if level is not None else "E0"
    else:
        body = f"精{elite}"
        if level is not None:
            body += f"/{level}级"
    skill = target.get("skill_name") or target.get("skill_id")
    if isinstance(skill, str) and skill.strip():
        body += f"（{skill.strip()}）"
    return body


def format_member(member: Any, context: str) -> str:
    member = require_object(member, context)
    name = require_string(member, "operator", context)
    role = require_string(member, "role", context)
    if role not in ROLES:
        raise RuleFormatError(f"{context}.role 非法: {role}")
    priority = require_string(member, "priority", context)
    if priority not in PRIORITIES:
        raise RuleFormatError(f"{context}.priority 非法: {priority}")
    target = format_target(member.get("target"), f"{context}.target")
    acquisition = member.get("acquisition", "policy")
    rarity = member.get("rarity")
    extra = []
    if acquisition != "policy":
        extra.append(f"获取={acquisition}")
    if is_int(rarity):
        extra.append(f"{rarity}星")
    suffix = f"（{'；'.join(extra)}）" if extra else ""
    return f"- `{role}` / {priority}：{name} → {target}{suffix}"


def render_rule(rule: Any, index: int) -> list[str]:
    rule = require_object(rule, f"rules[{index}]")
    rid = require_string(rule, "id", f"rules[{index}]")
    label = require_string(rule, "label", f"rules[{index}]")
    kind = require_string(rule, "kind", f"rules[{index}]")
    if kind not in KINDS:
        raise RuleFormatError(f"rules[{index}].kind 非法: {kind}")
    scope = require_string(rule, "scope", f"rules[{index}]")
    review = require_object(rule.get("review") or {}, f"rules[{index}].review")
    status = review.get("status", "confirmed")
    conflicts = review.get("conflicts") or []
    admission = require_object(rule.get("admission") or {}, f"rules[{index}].admission")
    required = admission.get("required_core") or []
    pick_one = admission.get("pick_one_core") or []
    required_groups = admission.get("required_core_groups") or []
    members = rule.get("members") or []
    evidence = rule.get("evidence") or []

    lines = [
        f"## {index + 1}. {label} (`{rid}`)",
        "",
        f"- 类型：`{kind}` / 范围：`{scope}`",
        f"- 复核：`{status}`",
    ]
    if rule.get("source_system_id"):
        lines.append(f"- source_system_id：`{rule['source_system_id']}`")
    if required:
        lines.append(f"- 必需核心：{', '.join(required)}")
    for slot in pick_one:
        slot = require_object(slot, f"rules[{index}].pick_one")
        lines.append(
            f"- 任选核心「{require_string(slot, 'label', 'pick_one')}」："
            + " / ".join(slot.get("candidates") or [])
        )
    for group in required_groups:
        group = require_object(group, f"rules[{index}].required_core_groups")
        count = group.get("required_count")
        if not is_int(count) or count < 2:
            raise RuleFormatError(
                f"rules[{index}].required_core_groups.required_count 必须是至少 2 的整数"
            )
        lines.append(
            f"- 核心组「{require_string(group, 'label', 'required_core_groups')}」"
            f"至少 {count} 人："
            + " / ".join(group.get("candidates") or [])
        )
    lines.append("- 成员：")
    for i, m in enumerate(members):
        lines.append(format_member(m, f"rules[{index}].members[{i}]"))
    if evidence:
        lines.append("- 来源：")
        for ev in evidence:
            ev = require_object(ev, "evidence")
            path = require_string(ev, "path", "evidence")
            heading = ev.get("heading")
            if isinstance(heading, str) and heading.strip():
                lines.append(f"  - {path} § {heading.strip()}")
            else:
                lines.append(f"  - {path}")
    if conflicts:
        lines.append("- 冲突：")
        for c in conflicts:
            lines.append(f"  - {c}")
    lines.append("")
    return lines


def render_rules(rules: dict[str, Any], source: str) -> str:
    policy = rules.get("acquisition_policy") or {}
    lines = [
        "# 基建练卡推荐规则验收稿",
        "",
        f"> 来源：`{source}`",
        "> 本文只投影旧 `training_recommendations.json`。玩家业务真源见 `docs/练卡推荐相关/`；v2 合同已归档。不要直接编辑本文或用本文覆盖玩家 canonical。",
        "",
        f"- schema version：{rules.get('version')}",
        f"- 规则数：{len(rules.get('rules') or [])}",
        f"- 获取策略：默认 ≤{policy.get('default_rarity_le', 4)} 星；"
        f"白名单：{', '.join(policy.get('named_exceptions') or []) or '无'}",
        "",
        "## 说明",
        "",
        "- 本稿只投影结构化规则，不包含账号过滤结果。",
        "- 不渲染面向用户的自由文案；解释应引用 evidence 原文。",
        "",
    ]
    for i, rule in enumerate(rules.get("rules") or []):
        lines.extend(render_rule(rule, i))
    return "\n".join(lines).rstrip() + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=DEFAULT_INPUT)
    parser.add_argument("--output", type=Path, default=None)
    args = parser.parse_args(argv)
    try:
        rules = load_rules(args.input)
        text = render_rules(rules, str(args.input))
    except RuleFormatError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
