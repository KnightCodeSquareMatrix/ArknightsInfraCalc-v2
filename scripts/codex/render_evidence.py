#!/usr/bin/env python3
"""Validate an evidence manifest and render final-response Markdown."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


CATEGORY_GROUPS = [
    ("Build", {"build"}),
    ("终态断言与批量 Box", {"invariants", "batch"}),
    ("真实 CLI", {"cli"}),
    ("性能", {"performance", "benchmark"}),
]
JSON_ARTIFACT_KINDS = {"profile", "maa", "json"}


class ManifestError(ValueError):
    pass


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ManifestError(f"cannot read manifest {path}: {error}") from error
    if not isinstance(value, dict) or value.get("schema_version") != 3:
        raise ManifestError("manifest must be a schema_version=3 JSON object")
    if not isinstance(value.get("runs"), list) or not isinstance(value.get("artifacts"), list):
        raise ManifestError("manifest runs and artifacts must be arrays")
    return value


def read_status(path: Path) -> dict[str, str]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError) as error:
        raise ManifestError(f"cannot read status file {path}: {error}") from error
    values: dict[str, str] = {}
    for line in lines:
        if "=" in line:
            key, value = line.split("=", 1)
            values[key] = value
    return values


def _last_metadata(text: str, key: str) -> str | None:
    matches = re.findall(rf"^{re.escape(key)}=(.*)$", text, re.MULTILINE)
    return matches[-1] if matches else None


def validate_manifest(manifest: dict[str, Any]) -> None:
    if manifest.get("schema_version") != 3:
        raise ManifestError("manifest must use schema_version=3")
    if "docs_impact" in manifest:
        raise ManifestError("manifest schema v3 does not allow docs_impact")
    seen_ids: set[str] = set()
    for index, run in enumerate(manifest["runs"]):
        if not isinstance(run, dict):
            raise ManifestError(f"run {index} is not an object")
        run_id = str(run.get("id", ""))
        if not run_id or run_id in seen_ids:
            raise ManifestError(f"missing or duplicate run id: {run_id!r}")
        seen_ids.add(run_id)

        log = Path(str(run.get("log", "")))
        status_file = Path(str(run.get("status_file", "")))
        if not log.is_file():
            raise ManifestError(f"run {run_id} log does not exist: {log}")
        if not status_file.is_file():
            raise ManifestError(f"run {run_id} status does not exist: {status_file}")

        status = read_status(status_file)
        expected_exit = str(run.get("exit_code"))
        expected_result = str(run.get("result"))
        if status.get("exit_code") != expected_exit:
            raise ManifestError(f"run {run_id} status exit_code disagrees with manifest")
        if status.get("result_summary") != expected_result:
            raise ManifestError(f"run {run_id} status result disagrees with manifest")
        if Path(status.get("log", "")).resolve(strict=False) != log.resolve(strict=False):
            raise ManifestError(f"run {run_id} status log path disagrees with manifest")

        try:
            log_text = log.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            raise ManifestError(f"cannot read run log {log}: {error}") from error
        if _last_metadata(log_text, "exit_code") != expected_exit:
            raise ManifestError(f"run {run_id} log exit_code disagrees with manifest")
        if _last_metadata(log_text, "result_summary") != expected_result:
            raise ManifestError(f"run {run_id} log result disagrees with manifest")

    for artifact in manifest["artifacts"]:
        if not isinstance(artifact, dict) or not artifact.get("kind") or not artifact.get("path"):
            raise ManifestError("each artifact requires kind and path")
        path = Path(str(artifact["path"]))
        if not path.exists():
            raise ManifestError(f"registered artifact does not exist: {path}")


def _link(label: str, path: str) -> str:
    escaped_label = label.replace("[", "\\[").replace("]", "\\]")
    target = str(Path(path).resolve(strict=False))
    if any(character.isspace() for character in target):
        target = f"<{target}>"
    return f"[{escaped_label}]({target})"


def _run_links(runs: list[dict[str, Any]]) -> str:
    links = []
    for run in runs:
        label = f"{run.get('stem', run.get('category', 'run'))} {run.get('result')}"
        if run.get("exit_code") != 0:
            label += f" (exit {run.get('exit_code')})"
        links.append(_link(label, str(run["log"])))
    return "；".join(links)


def render(manifest: dict[str, Any]) -> str:
    runs: list[dict[str, Any]] = manifest["runs"]
    artifacts: list[dict[str, Any]] = manifest["artifacts"]
    lines = ["### 验证证据", ""]
    grouped_categories = set().union(*(categories for _, categories in CATEGORY_GROUPS))
    for label, categories in CATEGORY_GROUPS:
        selected = [run for run in runs if run.get("category") in categories]
        if selected:
            value = _run_links(selected)
            lines.append(f"- {label}：{value}")
        else:
            lines.append(f"- {label}：未跑（manifest 未登记该类别）")

    other_runs = [run for run in runs if run.get("category") not in grouped_categories]
    if other_runs:
        lines.append(f"- 其他验证：{_run_links(other_runs)}")

    json_artifacts = [a for a in artifacts if a.get("kind") in JSON_ARTIFACT_KINDS]
    if json_artifacts:
        lines.append(
            "- 生成 JSON："
            + "；".join(_link(str(a["kind"]), str(a["path"])) for a in json_artifacts)
        )
    else:
        lines.append("- 生成 JSON：未跑（manifest 未登记 profile / MAA / JSON 产物）")

    scope = manifest.get("change_scope", {})
    expansions = manifest.get("scope_expansions", [])
    side_findings = manifest.get("side_findings", [])
    reviewer = manifest.get("reviewer", {})
    lines.extend(["", "### 任务范围", ""])
    lines.append(f"- 不变量：{scope.get('invariant') or '未声明'}")
    changed_paths = reviewer.get("changed_paths") or []
    lines.append(f"- 实际改动：{', '.join(changed_paths) if changed_paths else '未登记'}")
    if expansions:
        values = [
            f"{item.get('id', 'unnamed')}: {item.get('reason', '无理由')}"
            for item in expansions
            if isinstance(item, dict)
        ]
        lines.append(f"- 范围扩展：{'；'.join(values)}")
    else:
        lines.append("- 范围扩展：无")
    deferred = [
        str(item.get("summary", "unnamed"))
        for item in side_findings
        if isinstance(item, dict) and item.get("disposition") == "deferred"
    ]
    lines.append(f"- 未处理旁支发现：{'；'.join(deferred) if deferred else '无'}")
    return "\n".join(lines) + "\n"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        manifest = load_manifest(args.manifest)
        validate_manifest(manifest)
        markdown = render(manifest)
    except ManifestError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(markdown, encoding="utf-8")
    else:
        print(markdown, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
