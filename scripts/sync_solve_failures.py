#!/usr/bin/env python3
"""Sync, ledger and CLI-fingerprint lookup for solve-failure evidence.

Solve-failure cases (no user feedback) live under evidence/<date>-<family>/
with raw-runs/<run-id>/..., mirroring the role feedback/<date>/<case>/ plays
for user feedback. This script is the owner of that evidence tree:

    python3 scripts/sync_solve_failures.py
    python3 scripts/sync_solve_failures.py --no-sync
    python3 scripts/sync_solve_failures.py --cli-fingerprint <sha-prefix>
    python3 scripts/sync_solve_failures.py --match-build target/release/infra-cli

Default (no query flag) captures new failed solve runs from each service
cli-runs directory, then refreshes the generated index block in
evidence/TRACKING.md. Query flags skip sync unless --sync is also passed and
search only local evidence/ and feedback/ debug-bundle.json fingerprints.

The one-command feedback sync (scripts/sync_feedback.py) performs the same
failed-run capture by delegating here; either entry point keeps the evidence
tree up to date.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

from sync_feedback import (
    DEFAULT_LOCAL_EVIDENCE_DIR,
    DEFAULT_REMOTE_FEEDBACK_DIRS,
    DEFAULT_SSH_TARGET,
    ROOT,
    capture_failed_runs,
    json_file_data,
    remote_dir_exists,
)

LEDGER_BEGIN = "<!-- BEGIN GENERATED SOLVE-FAILURE EVIDENCE -->"
LEDGER_END = "<!-- END GENERATED SOLVE-FAILURE EVIDENCE -->"
FINGERPRINT_KEY = "solver_executable_sha256"


def print_summary(header: str, rows: list[str]) -> None:
    print(f"\n{header}:")
    if not rows:
        print("(none)")
    for row in rows:
        print(row, flush=True)


def sync_capture(
    target: str, remote_dirs: list[str], evidence_dir: Path, hours: int, password: str
) -> list[tuple[Path, int]]:
    with tempfile.TemporaryDirectory(prefix="solve-evidence-sync-") as temp:
        captured: list[tuple[Path, int]] = []
        for remote_dir in remote_dirs:
            if not remote_dir_exists(target, remote_dir, password):
                continue
            captured.extend(
                capture_failed_runs(
                    evidence_dir, target, remote_dir, Path(temp), password, hours
                )
            )
        return captured


def family_cases(evidence_dir: Path) -> list[Path]:
    if not evidence_dir.exists():
        return []
    return sorted(path for path in evidence_dir.iterdir() if path.is_dir())


def family_stats(family_dir: Path) -> dict:
    raw_root = family_dir / "raw-runs"
    runs = sorted(raw_root.iterdir()) if raw_root.is_dir() else []
    runs = [path for path in runs if path.is_dir()]
    first_error = ""
    fingerprints: set[str] = set()
    for run in runs:
        if not first_error:
            response = json_file_data(run / "serve-response.json")
            if isinstance(response, dict):
                error = response.get("error", {})
                if isinstance(error, dict) and isinstance(error.get("message"), str):
                    first_error = error["message"]
        fingerprint = bundle_fingerprint(run / "debug-bundle.json")
        if fingerprint:
            fingerprints.add(fingerprint)
    return {
        "runs": runs,
        "first_error": first_error,
        "fingerprints": sorted(fingerprints),
    }


def bundle_fingerprint(bundle_path: Path) -> str | None:
    if not bundle_path.is_file():
        return None
    data = json_file_data(bundle_path)
    value = find_key(data, FINGERPRINT_KEY) if isinstance(data, dict) else None
    if isinstance(value, str) and value.strip():
        return value.strip()
    return None


def find_key(node, key: str):
    if isinstance(node, dict):
        if key in node:
            return node[key]
        for value in node.values():
            found = find_key(value, key)
            if found is not None:
                return found
    elif isinstance(node, list):
        for value in node:
            found = find_key(value, key)
            if found is not None:
                return found
    return None


def ledger_rows(evidence_dir: Path) -> list[str]:
    rows: list[str] = []
    for family in family_cases(evidence_dir):
        stats = family_stats(family)
        if not stats["runs"]:
            continue
        label = family.relative_to(evidence_dir)
        error = stats["first_error"].replace("|", "/")[:64] or "?"
        fingerprints = ", ".join(sha[:8] for sha in stats["fingerprints"]) or "unknown"
        rows.append(f"| `{label}` | {len(stats['runs'])} | {error} | {fingerprints} |")
    return rows


LEDGER_TEMPLATE = """# Solve-Failure Evidence Ledger（求解失败证据台账）

> 文档角色：evidence
> 生命周期状态：current
> 当前真源：docs/质量与维护/QUALITY_AND_AUDIT.md
> 摘要：登记无用户反馈的求解失败证据族（`evidence/<日期>-<错误族>/raw-runs/`）。

与 `feedback/TRACKING.md` 分工：反馈台账记用户反馈；本台账记 solver 自身失败族（无反馈入口的失败 `serve` 运行）。

## 处置规则

1. 每族先本地回放：`evidence/<族>/replay.sh target/release/infra-cli <outdir>`，确认 `ok=false` 可复现。
2. 定位到最小层：CLI / 排班 / 候选 / 机制 / 数据；改代码前把复现命令记录到本行。
3. 状态与反馈台账同词表：`intake` / `reproduced` / `localized` / `fixing` / `closed` / `regressed` / `blocked`。
4. 新族出现时在下方「族清单」加一行标 `intake`；修复验证后更新为 `closed` 或 `regressed`。
5. `evidence/` 含真实练度盒与运行日志，保持 untracked；`raw-runs` 材料视为原始证据不改写。

## 族清单（人工维护）

| 族目录 | 状态 | 根因 / 处置 | 复现与证据 |
|--------|------|-------------|-----------|
| `2026-09-02-aic-plan-3004` | intake | 二班 `main_backup_12_12` no-solution；旧生产构建 `9dd38385…` 产生，最新构建回放仍复现 | `evidence/2026-09-02-aic-plan-3004/replay.sh` |
| `2026-09-02-belongs-to-multiple-teams` | intake | 二班 `main_backup_12_12` 多队冲突；构建 `509f8ac8…` 产生，本地 5/5 复现 | `evidence/2026-09-02-belongs-to-multiple-teams/replay.sh` |

## 自动捕获索引

<!-- BEGIN GENERATED SOLVE-FAILURE EVIDENCE -->
| 族目录 | 运行数 | 首错摘要 | CLI 指纹(8hex) |
|--------|-------:|---------|----------------|
<!-- END GENERATED SOLVE-FAILURE EVIDENCE -->
"""


def update_ledger(evidence_dir: Path) -> Path:
    ledger = evidence_dir / "TRACKING.md"
    if not ledger.is_file():
        ledger.write_text(LEDGER_TEMPLATE, encoding="utf-8")
    text = ledger.read_text(encoding="utf-8")
    block = "\n".join(ledger_rows(evidence_dir))
    section = f"{LEDGER_BEGIN}\n| 族目录 | 运行数 | 首错摘要 | CLI 指纹(8hex) |\n|--------|-------:|---------|----------------|\n{block}\n{LEDGER_END}"
    if LEDGER_BEGIN in text and LEDGER_END in text:
        updated = re.sub(
            re.escape(LEDGER_BEGIN) + r".*?" + re.escape(LEDGER_END),
            section,
            text,
            flags=re.DOTALL,
        )
    else:
        updated = text.rstrip() + "\n\n" + section + "\n"
    if updated != text:
        ledger.write_text(updated, encoding="utf-8")
    return ledger


def fingerprint_records(roots: list[Path]) -> list[dict]:
    records: list[dict] = []
    for root in roots:
        if not root.exists():
            continue
        for bundle in root.rglob("debug-bundle.json"):
            fingerprint = bundle_fingerprint(bundle)
            if not fingerprint:
                continue
            parent = bundle.parent
            try:
                label = str(parent.relative_to(root))
            except ValueError:
                label = str(parent)
            error = ""
            response = json_file_data(parent / "serve-response.json")
            if isinstance(response, dict):
                error_obj = response.get("error", {})
                if isinstance(error_obj, dict) and isinstance(error_obj.get("message"), str):
                    error = error_obj["message"]
            records.append(
                {
                    "root": root.name,
                    "label": label,
                    "fingerprint": fingerprint,
                    "error": error,
                }
            )
    return records


def query_fingerprint(prefix: str, roots: list[Path]) -> tuple[list[dict], int]:
    prefix = prefix.lower()
    matched = [
        record
        for record in fingerprint_records(roots)
        if record["fingerprint"].startswith(prefix)
    ]
    unknown = sum(
        1
        for bundle in (
            path for root in roots if root.exists() for path in root.rglob("debug-bundle.json")
        )
        if bundle_fingerprint(bundle) is None
    )
    return matched, unknown


def print_query_results(prefix: str, matched: list[dict], unknown: int) -> None:
    print(f"\nCLI fingerprint match: {prefix}*")
    print(f"matched={len(matched)} unknown-fingerprint-bundles={unknown}")
    if not matched:
        print("(none)")
        return
    by_family: dict[str, list[str]] = {}
    for record in sorted(matched, key=lambda item: (item["root"], item["label"])):
        key = record["label"].split("/raw-runs/", 1)[0]
        line = f"  {record['root']}/{record['label']}  {record['fingerprint'][:8]}"
        if record["error"]:
            line = f"{line}  {record['error'][:60]}"
        by_family.setdefault(key, []).append(line)
    for family, lines in by_family.items():
        print(f"- {family}")
        for line in lines:
            print(line)


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=DEFAULT_SSH_TARGET)
    parser.add_argument(
        "--remote-dir", action="append", default=[], help="repeatable; defaults to all services"
    )
    parser.add_argument(
        "--evidence-dir",
        type=Path,
        default=DEFAULT_LOCAL_EVIDENCE_DIR,
        help="local solve-failure evidence directory",
    )
    parser.add_argument("--failed-run-hours", type=int, default=72)
    parser.add_argument("--no-sync", action="store_true", help="skip remote capture")
    parser.add_argument(
        "--cli-fingerprint",
        metavar="SHA",
        help="query local evidence/ and feedback/ by solver_executable_sha256 prefix (>=8 hex)",
    )
    parser.add_argument(
        "--match-build",
        metavar="PATH",
        help="query by the SHA-256 of a local infra-cli build path",
    )
    return parser.parse_args(argv)


def local_roots(evidence_dir: Path) -> list[Path]:
    return [evidence_dir, ROOT / "feedback"]


def resolve_local_dir(path: Path) -> Path:
    expanded = path.expanduser()
    if expanded.is_absolute():
        return expanded
    return ROOT / expanded


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    evidence_dir = resolve_local_dir(args.evidence_dir)
    password = os.environ.get("SSH_PASSWORD", "")

    query_prefix = args.cli_fingerprint
    if args.match_build:
        build = Path(args.match_build).expanduser()
        if not build.is_file():
            print(f"Build file not found: {args.match_build}", flush=True)
            return 2
        digest = sha256_of(build)
        print(f"Build SHA-256: {digest}", flush=True)
        query_prefix = query_prefix or digest

    if query_prefix is not None:
        query_prefix = query_prefix.lower()
        if len(query_prefix) < 8 or not re.fullmatch(r"[0-9a-f]+", query_prefix):
            print("--cli-fingerprint must be a hex prefix of at least 8 characters.", flush=True)
            return 2
        matched, unknown = query_fingerprint(query_prefix, local_roots(evidence_dir))
        print_query_results(query_prefix, matched, unknown)

    if not args.no_sync:
        remote_dirs = args.remote_dir or DEFAULT_REMOTE_FEEDBACK_DIRS
        print(f"\nSSH target: {args.target}", flush=True)
        print(f"Local evidence: {evidence_dir}", flush=True)
        captured = sync_capture(
            args.target, remote_dirs, evidence_dir, args.failed_run_hours, password
        )
        print_summary(
            "Solve-failure families captured",
            [f"{family.relative_to(evidence_dir)}: +{count} run(s)" for family, count in captured],
        )
        ledger = update_ledger(evidence_dir)
        print(f"Ledger refreshed: {ledger.relative_to(ROOT)}", flush=True)
    elif query_prefix is None:
        print("No action requested; pass --cli-fingerprint/--match-build or drop --no-sync.", flush=True)
        return 2

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
