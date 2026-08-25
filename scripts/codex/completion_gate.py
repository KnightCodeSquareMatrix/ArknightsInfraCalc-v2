#!/usr/bin/env python3
"""Evaluate a harness task and its evidence manifest fail-closed."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

try:
    import jsonschema
except ImportError:  # pragma: no cover - the repository test environment supplies it
    jsonschema = None


PASS_STATUS = "verified"
PROOF_STATUSES = {"pending", "verified", "partial", "failed", "blocked", "not_run"}
TASK_STATES = {"queued", "running", "waiting", "succeeded", "failed", "cancelled", "expired", "blocked"}


class GateError(ValueError):
    pass


def load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise GateError(f"cannot read {label} {path}: {error}") from error
    if not isinstance(value, dict):
        raise GateError(f"{label} must be a JSON object")
    return value


def _record(value: dict[str, Any]) -> dict[str, Any]:
    if value.get("protocol_version") != "agent-harness-v0" or not isinstance(value.get("task"), dict):
        raise GateError("task must be an agent-harness-v0 envelope; legacy top-level wave is rejected")
    record = value["task"]
    if jsonschema is None:
        raise GateError("jsonschema dependency is required for completion gate")
    schema_path = Path(__file__).parents[2] / "contracts" / "agent-harness-v0.schema.json"
    try:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        jsonschema.Draft202012Validator(schema).validate(value)
    except (OSError, UnicodeError, json.JSONDecodeError, jsonschema.ValidationError) as error:
        raise GateError(f"agent-harness-v0 schema validation failed: {error}") from error
    required = {
        "id",
        "protocol_version",
        "project_profile",
        "state",
        "write_policy",
        "data_classification",
        "proof_obligations",
        "children",
    }
    missing = sorted(required - set(record))
    if missing:
        raise GateError(f"task is missing required fields: {', '.join(missing)}")
    if record["protocol_version"] != "agent-harness-v0":
        raise GateError("task.protocol_version must be agent-harness-v0")
    if record["state"] not in TASK_STATES:
        raise GateError(f"invalid task.state: {record['state']!r}")
    if record["state"] != "succeeded":
        raise GateError("task.state must be succeeded before completion can pass")
    base_sha = record.get("base_sha")
    if not isinstance(base_sha, str) or len(base_sha) != 40 or any(char not in "0123456789abcdef" for char in base_sha):
        raise GateError("task.base_sha must be a 40-character lowercase commit SHA")
    if not isinstance(record.get("dirty_worktree"), bool):
        raise GateError("task.dirty_worktree must be boolean")
    if record["dirty_worktree"]:
        raise GateError("dirty worktree cannot pass completion gate")
    if record["write_policy"] != "readonly" and not record.get("approval_required"):
        raise GateError("non-readonly task requires approval_required=true")
    return record


def _check_approval(record: dict[str, Any]) -> None:
    if not record.get("approval_required"):
        return
    approval = record.get("approval")
    if not isinstance(approval, dict):
        raise GateError("approval is required for this task")
    required = {"decision_id", "approver", "approved_at", "scope", "scope_hash", "reason"}
    missing = sorted(required - set(approval))
    if missing:
        raise GateError(f"approval is missing required fields: {', '.join(missing)}")
    canonical = json.dumps(approval["scope"], ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    expected = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    if approval["scope_hash"] != expected:
        raise GateError("approval.scope_hash does not match approval.scope")


def _check_manifest(record: dict[str, Any], manifest: dict[str, Any] | None) -> dict[str, str]:
    if manifest is None:
        if any(obligation.get("evidence_refs") for obligation in record["proof_obligations"] if isinstance(obligation, dict)):
            raise GateError("manifest is required when proof obligations reference evidence")
        return {}
    if manifest.get("schema_version") != 3:
        raise GateError("evidence manifest schema_version must be 3")
    manifest_task = manifest.get("task")
    if not isinstance(manifest_task, dict) or manifest_task.get("base_sha") != record.get("base_sha"):
        raise GateError("manifest task base_sha must match task base_sha")
    refs: dict[str, str] = {}
    for run in manifest.get("runs", []):
        if not isinstance(run, dict) or not isinstance(run.get("id"), str):
            raise GateError("manifest runs must have string ids")
        refs[run["id"]] = "run_pass" if run.get("result") == "PASS" else "run_fail"
        for field in ("base_sha", "input_sha256", "command_sha256"):
            if not isinstance(run.get(field), str) or not run[field]:
                raise GateError(f"run {run['id']} is missing {field}")
        if run["base_sha"] != manifest_task.get("base_sha"):
            raise GateError(f"run {run['id']} base SHA does not match manifest")
        if run.get("result") == "PASS":
            for field in ("log", "status_file"):
                if not Path(str(run.get(field, ""))).is_file():
                    raise GateError(f"PASS run {run['id']} is missing {field}")
            log_text = Path(run["log"]).read_text(encoding="utf-8")
            status_text = Path(run["status_file"]).read_text(encoding="utf-8")
            if "result_summary=PASS" not in log_text or "exit_code=0" not in log_text or "result_summary=PASS" not in status_text or "exit_code=0" not in status_text:
                raise GateError(f"PASS run {run['id']} log/status is inconsistent")
        if run.get("result") != "PASS":
            continue
    for artifact in manifest.get("artifacts", []):
        if not isinstance(artifact, dict) or not isinstance(artifact.get("id"), str):
            raise GateError("manifest artifacts must have stable ids")
        refs[artifact["id"]] = "artifact"
        path = Path(str(artifact.get("path", "")))
        expected = artifact.get("sha256")
        if not path.is_file() or not isinstance(expected, str) or len(expected) != 64:
            raise GateError(f"artifact {artifact['id']} is missing or has no digest")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            raise GateError(f"artifact {artifact['id']} digest mismatch")
        for field in ("producer", "retention", "visibility", "run_id"):
            if not artifact.get(field):
                raise GateError(f"artifact {artifact['id']} is missing {field}")
        for field in ("base_sha", "input_sha256", "command_sha256"):
            if not isinstance(artifact.get(field), str) or not artifact[field]:
                raise GateError(f"artifact {artifact['id']} is missing {field}")
    expected_ref_count = len(manifest.get("runs", [])) + len(manifest.get("artifacts", []))
    if len(refs) != expected_ref_count:
        raise GateError("manifest contains duplicate run or artifact ids")
    runs_by_id = {run["id"]: run for run in manifest.get("runs", [])}
    for artifact in manifest.get("artifacts", []):
        run = runs_by_id.get(artifact.get("run_id"))
        if run is None:
            raise GateError(f"artifact {artifact['id']} is not bound to a manifest run")
        if run.get("result") != "PASS" or run.get("exit_code") != 0:
            raise GateError(f"artifact {artifact['id']} is bound to a non-PASS run")
        for field in ("base_sha", "input_sha256", "command_sha256"):
            if artifact.get(field) and artifact[field] != run.get(field):
                raise GateError(f"artifact {artifact['id']} {field} does not match its run")
    cwd = manifest_task.get("cwd")
    if not cwd:
        raise GateError("evidence manifest task.cwd is required for current HEAD verification")
    try:
        current_head = subprocess.run(
            ["git", "-C", str(cwd), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise GateError(f"cannot verify current HEAD for evidence manifest: {error}") from error
    if current_head != manifest_task.get("base_sha"):
        raise GateError("current HEAD does not match evidence manifest base_sha")
    return refs


def evaluate(task: dict[str, Any], manifest: dict[str, Any] | None = None) -> dict[str, Any]:
    record = _record(task)
    _check_approval(record)
    refs = _check_manifest(record, manifest)
    obligations = record["proof_obligations"]
    if not isinstance(obligations, list):
        raise GateError("task.proof_obligations must be an array")

    required: list[dict[str, Any]] = []
    for index, obligation in enumerate(obligations):
        if not isinstance(obligation, dict):
            raise GateError(f"proof_obligations[{index}] must be an object")
        if not isinstance(obligation.get("id"), str) or not obligation["id"]:
            raise GateError(f"proof_obligations[{index}].id must be non-empty")
        if not isinstance(obligation.get("required"), bool):
            raise GateError(f"proof_obligations[{index}].required must be boolean")
        if obligation.get("status") not in PROOF_STATUSES:
            raise GateError(f"proof_obligations[{index}].status must be a valid proof state")
        evidence_refs = obligation.get("evidence_refs", [])
        if not isinstance(evidence_refs, list) or not all(isinstance(ref, str) and ref in refs for ref in evidence_refs):
            raise GateError(f"proof_obligations[{index}].evidence_refs must reference manifest evidence")
        if obligation.get("status") == PASS_STATUS and not evidence_refs:
            raise GateError(f"proof_obligations[{index}] cannot be verified without evidence_refs")
        if len(evidence_refs) != len(set(evidence_refs)):
            raise GateError(f"proof_obligations[{index}].evidence_refs contains duplicates")
        if obligation.get("status") == PASS_STATUS and any(refs[ref] == "run_fail" for ref in evidence_refs):
            raise GateError(f"proof_obligations[{index}] references a failed evidence run")
        for ref in evidence_refs:
            artifact = next((item for item in (manifest or {}).get("artifacts", []) if item.get("id") == ref), None)
            if obligation.get("status") == PASS_STATUS and artifact is not None:
                proof = Path(str(artifact["path"]))
                try:
                    proof_value = json.loads(proof.read_text(encoding="utf-8"))
                except (OSError, UnicodeError, json.JSONDecodeError) as error:
                    raise GateError(f"verified artifact {ref} is not a proof-status JSON: {error}") from error
                if not isinstance(proof_value, dict) or proof_value.get("artifact_type") != "proof_status" or proof_value.get("obligation_id") != obligation["id"] or proof_value.get("proof_status") != "verified":
                    raise GateError(f"verified artifact {ref} does not prove obligation {obligation['id']}")
        if obligation["required"]:
            required.append(obligation)

    failures = [
        {"id": obligation["id"], "status": obligation["status"], "reason": "required obligation is not verified"}
        for obligation in required
        if obligation["status"] != PASS_STATUS
    ]
    return {
        "status": "verified" if not failures else "blocked",
        "required_count": len(required),
        "verified_count": sum(obligation["status"] == PASS_STATUS for obligation in required),
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--task", type=Path, required=True)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = evaluate(load_json(args.task, "task"), load_json(args.manifest, "manifest") if args.manifest else None)
    except GateError as error:
        print(f"completion gate error: {error}", file=sys.stderr)
        return 2
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    print(f"completion_gate={result['status']}", file=sys.stderr)
    return 0 if result["status"] == "verified" else 1


if __name__ == "__main__":
    raise SystemExit(main())
