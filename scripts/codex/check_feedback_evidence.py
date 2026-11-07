#!/usr/bin/env python3
"""Validate tracked feedback ledger metadata against local-only bundles."""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
from dataclasses import dataclass
from pathlib import Path

import doc_layout


CASE_ID_RE = re.compile(r"^FB-\d{8}-\d{6}$")
DIGEST_RE = re.compile(r"^[0-9a-f]{64}$")
MARKERS = ("<!-- BEGIN GENERATED LOCAL EVIDENCE -->", "<!-- END GENERATED LOCAL EVIDENCE -->")
LOCAL_RE = re.compile(r"^`local-evidence:([^`]+)`$")


@dataclass(frozen=True)
class Case:
    case_id: str
    status: str
    source: str
    closure: str


def bundle_digest(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(b"feedback-bundle-v1\0")
    files = sorted((item for item in path.rglob("*") if item.is_file()), key=lambda item: item.relative_to(path).as_posix())
    for item in files:
        relative = item.relative_to(path).as_posix()
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(item.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def parse_cases(text: str) -> list[Case]:
    cases: list[Case] = []
    in_ledger = False
    for line in text.splitlines():
        if line == "## Case Ledger":
            in_ledger = True
            continue
        if in_ledger and line.startswith("## "):
            break
        if not in_ledger or not line.startswith("| FB-"):
            continue
        columns = [column.strip() for column in line.strip().strip("|").split("|")]
        if len(columns) != 5:
            raise ValueError(f"feedback case row must contain five columns: {line}")
        cases.append(Case(columns[0], columns[1], columns[3], columns[4]))
    return cases


def parse_generated_ledger(text: str) -> dict[str, tuple[str, str]]:
    begin, end = MARKERS
    if begin not in text or end not in text:
        raise ValueError("feedback local-evidence generated markers are missing")
    body = text.split(begin, 1)[1].split(end, 1)[0]
    ledger: dict[str, tuple[str, str]] = {}
    for line in body.splitlines():
        if not line.startswith("| FB-"):
            continue
        columns = [column.strip() for column in line.strip().strip("|").split("|")]
        if len(columns) != 3:
            raise ValueError(f"feedback digest row must contain three columns: {line}")
        if columns[0] in ledger:
            raise ValueError(f"duplicate feedback digest row: {columns[0]}")
        ledger[columns[0]] = (columns[1].strip("`"), columns[2].strip("`"))
    return ledger


def check_tracking(repo: Path, *, verify_local: bool) -> list[str]:
    path = repo / "feedback/TRACKING.md"
    text = path.read_text(encoding="utf-8")
    errors: list[str] = []
    try:
        cases = parse_cases(text)
        ledger = parse_generated_ledger(text)
    except ValueError as error:
        return [str(error)]
    case_ids = [case.case_id for case in cases]
    if len(case_ids) != len(set(case_ids)):
        errors.append("feedback case IDs must be unique")
    if set(case_ids) != set(ledger):
        errors.append("feedback generated digest ledger must cover every case exactly once")
    for case in cases:
        if not CASE_ID_RE.fullmatch(case.case_id):
            errors.append(f"invalid feedback case ID: {case.case_id}")
        source_match = LOCAL_RE.fullmatch(case.source)
        if source_match is None:
            errors.append(f"feedback source must use local-evidence identifier: {case.case_id}")
            continue
        digest, availability = ledger.get(case.case_id, ("", ""))
        if not DIGEST_RE.fullmatch(digest):
            errors.append(f"invalid feedback bundle digest: {case.case_id}")
        if availability != "local-only":
            errors.append(f"feedback availability must be local-only: {case.case_id}")
        if case.status not in {"closed", "duplicate-covered"}:
            targets = re.findall(r"docs/TODO/详情/[^\s`;|)]+\.md", case.closure)
            if not targets:
                errors.append(f"open feedback must link an active change detail: {case.case_id}")
            for target in targets:
                if not target.startswith(doc_layout.ACTIVE_DETAILS_ROOT + "/") or not (repo / target).is_file():
                    errors.append(f"open feedback has invalid active detail: {case.case_id}: {target}")
        if verify_local:
            bundle = repo / "feedback" / source_match.group(1)
            if not bundle.is_dir():
                errors.append(f"local feedback bundle is unavailable: {case.case_id}: {bundle}")
            elif bundle_digest(bundle) != digest:
                errors.append(f"local feedback bundle digest drift: {case.case_id}")
    return errors


def _render_ledger(repo: Path, cases: list[Case]) -> str:
    lines = [
        MARKERS[0],
        "| ID | Bundle SHA-256 | Availability |",
        "|---|---|---|",
    ]
    for case in cases:
        match = LOCAL_RE.fullmatch(case.source)
        if match is None:
            raise ValueError(f"case source is not normalized: {case.case_id}")
        bundle = repo / "feedback" / match.group(1)
        if not bundle.is_dir():
            raise ValueError(f"local feedback bundle is unavailable: {case.case_id}: {bundle}")
        lines.append(f"| {case.case_id} | `{bundle_digest(bundle)}` | `local-only` |")
    lines.append(MARKERS[1])
    return "\n".join(lines)


def write_tracking(repo: Path) -> None:
    path = repo / "feedback/TRACKING.md"
    text = path.read_text(encoding="utf-8")
    folder_re = re.compile(r"\[folder\]\(([^)]+)\)")
    text = folder_re.sub(lambda match: f"`local-evidence:{match.group(1)}`", text)
    cases = parse_cases(text)
    rendered = _render_ledger(repo, cases)
    begin, end = MARKERS
    if begin in text and end in text:
        text = re.sub(re.escape(begin) + r".*?" + re.escape(end), rendered, text, flags=re.DOTALL)
    else:
        marker = "## Closure Buckets"
        if marker not in text:
            raise ValueError("cannot place generated feedback ledger")
        text = text.replace(marker, f"## Local Evidence Digests\n\n{rendered}\n\n{marker}", 1)
    path.write_text(text, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--verify-local", action="store_true")
    args = parser.parse_args()
    repo = args.repo_root.resolve()
    try:
        if args.write:
            write_tracking(repo)
        errors = check_tracking(repo, verify_local=args.verify_local)
    except (OSError, UnicodeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"PASS feedback_evidence local={'verified' if args.verify_local else 'not-checked'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
