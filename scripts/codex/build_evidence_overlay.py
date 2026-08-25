#!/usr/bin/env python3
"""Build a read-only evidence overlay from a task and evidence manifest."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

try:
    import jsonschema
except ImportError:  # pragma: no cover
    jsonschema = None

from completion_gate import GateError, _check_manifest


class OverlayError(ValueError):
    pass


PROOF_STATUSES = {"pending", "verified", "partial", "failed", "blocked", "not_run"}


def load_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise OverlayError(f"cannot read {label} {path}: {error}") from error
    if not isinstance(value, dict):
        raise OverlayError(f"{label} must be a JSON object")
    return value


def task_record(value: dict[str, Any]) -> dict[str, Any]:
    if value.get("protocol_version") != "agent-harness-v0" or not isinstance(value.get("task"), dict):
        raise OverlayError("task must be an agent-harness-v0 envelope; legacy top-level wave is rejected")
    if jsonschema is None:
        raise OverlayError("jsonschema dependency is required for evidence overlay")
    schema_path = Path(__file__).parents[2] / "contracts" / "agent-harness-v0.schema.json"
    try:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        jsonschema.Draft202012Validator(schema).validate(value)
    except (OSError, UnicodeError, json.JSONDecodeError, jsonschema.ValidationError) as error:
        raise OverlayError(f"agent-harness-v0 schema validation failed: {error}") from error
    return value["task"]


def proof_artifact_status(path: str, obligation_id: str) -> str | None:
    candidate = Path(path)
    if candidate.suffix != ".json" or not candidate.is_file():
        return None
    try:
        value = json.loads(candidate.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        return None
    if not isinstance(value, dict):
        return None
    if value.get("artifact_type") != "proof_status" or value.get("obligation_id") != obligation_id:
        return None
    status = value.get("proof_status")
    return status if status in PROOF_STATUSES else None


def build_overlay(task: dict[str, Any], manifest: dict[str, Any], manifest_path: Path) -> dict[str, Any]:
    record = task_record(task)
    if record.get("state") != "succeeded" or record.get("dirty_worktree") is not False:
        raise OverlayError("task must be succeeded with dirty_worktree=false")
    try:
        _check_manifest(record, manifest)
    except GateError as error:
        raise OverlayError(f"evidence manifest validation failed: {error}") from error
    obligations = record.get("proof_obligations")
    runs = manifest.get("runs")
    artifacts = manifest.get("artifacts")
    if not isinstance(obligations, list):
        raise OverlayError("task proof_obligations must be an array")
    if not isinstance(runs, list) or not isinstance(artifacts, list):
        raise OverlayError("manifest runs and artifacts must be arrays")

    entries: list[dict[str, Any]] = []
    run_by_id = {run.get("id"): run for run in runs if isinstance(run, dict) and run.get("id")}
    artifact_by_id = {
        artifact.get("id"): artifact
        for artifact in artifacts
        if isinstance(artifact, dict) and artifact.get("id")
    }
    artifact_by_path = {
        artifact.get("path"): artifact
        for artifact in artifacts
        if isinstance(artifact, dict) and artifact.get("path")
    }

    for index, obligation in enumerate(obligations):
        if not isinstance(obligation, dict) or not isinstance(obligation.get("id"), str):
            raise OverlayError(f"proof_obligations[{index}] must have a string id")
        refs = obligation.get("evidence_refs", [])
        if not isinstance(refs, list) or not all(isinstance(ref, str) and ref for ref in refs):
            raise OverlayError(f"proof_obligations[{index}].evidence_refs must be string array")

        evidence: list[dict[str, Any]] = []
        missing: list[str] = []
        failures: list[str] = []
        blocked: list[str] = []
        for ref in refs:
            run = run_by_id.get(ref)
            artifact = artifact_by_id.get(ref) or artifact_by_path.get(ref)
            if run is not None:
                if run.get("result") == "PASS" and (
                    not Path(str(run.get("log", ""))).is_file()
                    or not Path(str(run.get("status_file", ""))).is_file()
                ):
                    raise OverlayError(f"PASS run {ref} is missing log or status file")
                evidence.append({"ref": ref, "kind": "run", "result": run.get("result"), "log": run.get("log")})
                if run.get("result") != "PASS":
                    failures.append(ref)
            elif artifact is not None:
                artifact_path = str(artifact.get("path"))
                proof_status = proof_artifact_status(artifact_path, obligation["id"])
                evidence.append(
                    {
                        "ref": ref,
                        "kind": "proof_artifact" if proof_status else "artifact",
                        "status": proof_status or "partial",
                        "path": artifact_path,
                    }
                )
                if proof_status == "blocked":
                    blocked.append(ref)
                elif proof_status in {"failed", "not_run", "partial"}:
                    failures.append(ref)
            else:
                missing.append(ref)

        if not refs:
            status = "not_run"
        elif missing:
            status = "blocked"
        elif blocked:
            status = "blocked"
        elif failures or any(item.get("status") == "pending" for item in evidence):
            status = "failed"
        elif evidence and any(item.get("status") == "partial" for item in evidence):
            status = "partial"
        else:
            status = "verified"
        entries.append(
            {
                "id": obligation["id"],
                "required": bool(obligation.get("required", False)),
                "status": status,
                "evidence_refs": refs,
                "evidence": evidence,
                "missing_refs": missing,
                "failed_refs": failures,
                "blocked_refs": blocked,
            }
        )

    required_failures = [entry["id"] for entry in entries if entry["required"] and entry["status"] != "verified"]
    return {
        "protocol_version": "agent-harness-v0",
        "overlay_version": "evidence-overlay-v0",
        "task_id": record.get("id") or task.get("wave_id") or manifest.get("task", {}).get("slug"),
        "manifest": str(manifest_path.resolve(strict=False)),
        "status": "verified" if not required_failures else "blocked",
        "required_failures": required_failures,
        "obligations": entries,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--task", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        overlay = build_overlay(
            load_object(args.task, "task"),
            load_object(args.manifest, "manifest"),
            args.manifest,
        )
    except OverlayError as error:
        print(f"evidence overlay error: {error}", file=sys.stderr)
        return 2

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(overlay, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"evidence_overlay={overlay['status']}", file=sys.stderr)
    return 0 if overlay["status"] == "verified" else 1


if __name__ == "__main__":
    raise SystemExit(main())
