#!/usr/bin/env python3
"""Build a fail-closed, read-only Phase 0C research-wave plan."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


PROTOCOL_VERSION = "agent-harness-v0"
PROJECT_PROFILE = "arknights-infra-calc-solver"


CHILDREN = [
    {
        "id": "layout-owner",
        "profile": "terra-explorer",
        "role": "domain_explorer",
        "prompt": "调查 layout 编排主路径，确认 Rule/Plan/Execute/Fill/Resolve 的当前 owner、调用链和 canonical 入口。只读，报告文件、行号、事实、反例和未知项。",
        "required_output": ["claims", "sources", "unknowns", "negative_evidence"],
    },
    {
        "id": "cross-facility-owner",
        "profile": "terra-explorer",
        "role": "domain_explorer",
        "prompt": "调查 cross_facility 的当前 owner、scope=Global atom 执行路径和与 resolve_base 的关系。只读，报告文件、行号、事实、冲突和未知项。",
        "required_output": ["claims", "sources", "unknowns", "canonical_conflicts"],
    },
    {
        "id": "evidence-entry",
        "profile": "luna-extractor",
        "role": "evidence_researcher",
        "prompt": "提取当前 evidence 工具入口、验证类别、manifest/artifact 路径和完成检查。只读，不运行命令，报告稳定字段、来源和未找到项。",
        "required_output": ["claims", "sources", "unknowns", "evidence_refs"],
    },
]


def git_value(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def build_plan(root: Path, *, allow_dirty_readonly: bool) -> dict[str, Any]:
    base_sha = git_value(root, "rev-parse", "HEAD")
    dirty = bool(git_value(root, "status", "--porcelain"))
    blocked_reason = ""
    if dirty and not allow_dirty_readonly:
        blocked_reason = "worktree is dirty; pass --allow-dirty-readonly for public read-only planning"

    children = [
        {
            "id": child["id"],
            "role": child["role"],
            "task": child["prompt"],
            "state": "queued",
            "write_policy": "readonly",
            "profile": child["profile"],
        }
        for child in CHILDREN
    ]
    child_ids = [child["id"] for child in children]
    obligations = [
        {
            "id": "all_children_return",
            "kind": "structure",
            "required": True,
            "status": "pending",
            "notes": "All queued child tasks must return a ResultContract-compatible result.",
        },
        {
            "id": "public_scope_only",
            "kind": "privacy",
            "required": True,
            "status": "pending",
            "notes": "No private operbox/layout/assignment input or remote private artifact is allowed.",
        },
        {
            "id": "join_review",
            "kind": "review",
            "required": True,
            "status": "pending",
            "notes": "The parent must review claims, canonical conflicts and unknowns before synthesis.",
        },
    ]
    return {
        "protocol_version": PROTOCOL_VERSION,
        "task": {
            "id": "research-wave-0c",
            "protocol_version": PROTOCOL_VERSION,
            "project_profile": PROJECT_PROFILE,
            "state": "blocked" if blocked_reason else "queued",
            "write_policy": "readonly",
            "data_classification": "public",
            "base_sha": base_sha,
            "dirty_worktree": dirty,
            "profile": "research-wave",
            "resource_class": "readonly",
            "children": children,
            "proof_obligations": obligations,
            "research_wave": {
                "protocol_version": "research-wave-v0",
                "wave_id": "research-wave-0c",
                "scope": {
                    "writes": False,
                    "private_inputs": False,
                    "business_commands": False,
                    "runtime_start": False,
                    "data_classification": "public",
                },
                "join_policy": {
                    "mode": "all_required",
                    "required_children": child_ids,
                    "fail_closed": True,
                },
                "result_contracts": [
                    {"child_id": child["id"], "required_output": child["required_output"]}
                    for child in CHILDREN
                ],
            },
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path)
    parser.add_argument("--allow-dirty-readonly", action="store_true")
    args = parser.parse_args()

    try:
        plan = build_plan(args.root.resolve(), allow_dirty_readonly=args.allow_dirty_readonly)
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"research-wave planning error: {error}", file=sys.stderr)
        return 2

    rendered = json.dumps(plan, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    return 0 if plan["task"]["state"] == "queued" else 1


if __name__ == "__main__":
    raise SystemExit(main())
