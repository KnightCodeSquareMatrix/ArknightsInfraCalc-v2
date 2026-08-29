#!/usr/bin/env python3
"""Export self-contained quality feedback packages from the Check server."""

from __future__ import annotations

import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


REMOTE_HOST = "root@114.66.55.78"
REMOTE_KEY = Path.home() / ".ssh" / "id_rsa"
OUTPUT_ROOT = Path(__file__).resolve().parent.parent / "output" / "feedbacks"
SEVERITIES = {"critical", "moderate", "minor"}

REMOTE_SCRIPT = r'''import glob, json, os

root = "/var/lib/arknights-infra-quality/v2"
pattern = os.path.join(root, "job-data", "*", "feedback", "*", "*", "feedback.json")
for metadata_path in sorted(glob.glob(pattern)):
    package_dir = os.path.dirname(metadata_path)
    metadata = json.load(open(metadata_path, encoding="utf-8"))
    package = {
        "feedback": metadata,
        "layout": json.load(open(os.path.join(package_dir, metadata["layoutFile"]), encoding="utf-8")),
        "operbox": json.load(open(os.path.join(package_dir, metadata["operboxFile"]), encoding="utf-8")),
        "planRequest": json.load(open(os.path.join(package_dir, metadata["planRequestFile"]), encoding="utf-8")),
    }
    print(json.dumps(package, ensure_ascii=False, separators=(",", ":")))
'''


def fetch_feedback() -> list[dict]:
    result = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-i", str(REMOTE_KEY), REMOTE_HOST, "python3", "-"],
        input=REMOTE_SCRIPT,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or f"ssh exited with status {result.returncode}")
    records = []
    for line_number, line in enumerate(result.stdout.splitlines(), start=1):
        try:
            value = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"remote output line {line_number} is not JSON: {error}") from error
        if not isinstance(value, dict):
            raise RuntimeError(f"remote output line {line_number} is not an object")
        records.append(value)
    return records


def existing_ids() -> set[str]:
    ids: set[str] = set()
    if not OUTPUT_ROOT.exists():
        return ids
    for path in OUTPUT_ROOT.rglob("feedback.json"):
        value = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(value, dict) and isinstance(value.get("feedbackId"), str):
            ids.add(value["feedbackId"])
    return ids


def parse_timestamp(value: object) -> tuple[datetime, str]:
    if not isinstance(value, str) or not value.endswith("Z"):
        raise RuntimeError("feedback createdAt must be an ISO timestamp ending in Z")
    try:
        parsed = datetime.fromisoformat(value[:-1] + "+00:00").astimezone(timezone.utc)
    except ValueError as error:
        raise RuntimeError(f"feedback createdAt is invalid: {value}") from error
    return parsed, parsed.strftime("%Y-%m-%d_%H-%M-%S_%f")[:-3] + "Z"


def export(records: list[dict]) -> tuple[int, int]:
    known = existing_ids()
    written = skipped = 0
    for record in records:
        feedback = record.get("feedback")
        if not isinstance(feedback, dict):
            raise RuntimeError("feedback metadata is not an object")
        feedback_id = feedback.get("feedbackId")
        severity = feedback.get("severity")
        if not isinstance(feedback_id, str) or not feedback_id:
            raise RuntimeError("feedbackId is missing")
        if severity not in SEVERITIES:
            raise RuntimeError("feedback severity is invalid")
        if feedback_id in known:
            skipped += 1
            continue
        created, timestamp_name = parse_timestamp(feedback.get("createdAt"))
        target = OUTPUT_ROOT / created.strftime("%Y") / created.strftime("%m") / severity / f"{timestamp_name}_{feedback_id}"
        target.mkdir(parents=True, exist_ok=True)
        for name, key in [("feedback.json", "feedback"), ("layout.json", "layout"), ("operbox.json", "operbox"), ("plan-request.json", "planRequest")]:
            (target / name).write_text(json.dumps(record[key], ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        known.add(feedback_id)
        written += 1
    return written, skipped


def main() -> int:
    if len(sys.argv) != 1:
        print("此脚本不接受参数。", file=sys.stderr)
        return 2
    try:
        records = fetch_feedback()
        written, skipped = export(records)
    except (OSError, RuntimeError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"反馈提取失败：{error}", file=sys.stderr)
        return 1
    print(f"远端反馈包 {len(records)} 个；新写入 {written} 个；已存在跳过 {skipped} 个。")
    print(f"输出目录：{OUTPUT_ROOT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
