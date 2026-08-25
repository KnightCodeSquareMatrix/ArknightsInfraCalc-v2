#!/usr/bin/env python3
"""Atomic manifest updates for run_evidence.sh."""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
import shlex
import tempfile
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 3


def _absolute(path: str, cwd: Path) -> str:
    candidate = Path(path)
    if not candidate.is_absolute():
        candidate = cwd / candidate
    return str(candidate.resolve(strict=False))


def _empty_manifest(task: str, base_sha: str, cwd: Path, created_at: str) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "task": {
            "slug": task,
            "base_sha": base_sha,
            "cwd": str(cwd),
            "created_at": created_at,
        },
        "change_scope": {
            "invariant": "",
            "root_cause_layer": "",
            "required_paths": [],
            "allowed_consumers": [],
            "proof_paths": [],
            "explicitly_deferred": [],
        },
        "scope_expansions": [],
        "side_findings": [],
        "reviewer": {
            "status": "pending",
            "scope_invariant": "",
            "changed_paths": [],
            "scope_expansion_ids": [],
        },
        "runs": [],
        "artifacts": [],
    }


def _load_metadata(path: str | None) -> dict[str, Any]:
    if not path:
        return {}
    with Path(path).open(encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError("task metadata must be a JSON object")
    return value


def _merge_metadata(manifest: dict[str, Any], metadata: dict[str, Any]) -> None:
    allowed = {
        "change_scope",
        "scope_expansions",
        "side_findings",
        "reviewer",
        "approval",
    }
    unknown = sorted(set(metadata) - allowed)
    if unknown:
        raise ValueError(f"unsupported task metadata keys: {', '.join(unknown)}")
    for key in allowed:
        if key in metadata:
            manifest[key] = metadata[key]


def _read_manifest(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"manifest is not a JSON object: {path}")
    if value.get("schema_version") != SCHEMA_VERSION:
        raise ValueError(f"unsupported manifest schema: {value.get('schema_version')!r}")
    return value


def _atomic_write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def _parse_artifact(value: str, cwd: Path, run_id: str, ordinal: int) -> dict[str, str]:
    if "=" not in value:
        raise ValueError(f"artifact must use kind=path syntax: {value!r}")
    kind, path = value.split("=", 1)
    if not kind or not path:
        raise ValueError(f"artifact must use non-empty kind=path syntax: {value!r}")
    absolute = Path(_absolute(path, cwd))
    digest = hashlib.sha256(absolute.read_bytes()).hexdigest() if absolute.is_file() else ""
    artifact_id = f"artifact:{run_id}:{ordinal}:{kind}"
    return {
        "id": artifact_id,
        "kind": kind,
        "path": str(absolute),
        "sha256": digest,
        "visibility": "project",
        "retention": "run",
        "producer": "scripts/codex/run_evidence.sh",
        "run_id": run_id,
        "status": "registered" if digest else "missing",
    }


def _parse_command_json(value: str) -> list[str]:
    command = json.loads(value)
    if not isinstance(command, list) or not command or not all(isinstance(arg, str) for arg in command):
        raise argparse.ArgumentTypeError("--command-json must encode a non-empty string array")
    return command


def append_run(args: argparse.Namespace) -> None:
    manifest_path = Path(args.manifest).resolve(strict=False)
    cwd = Path(args.cwd).resolve(strict=False)
    lock_path = manifest_path.with_suffix(f"{manifest_path.suffix}.lock")
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    metadata = _load_metadata(args.metadata)

    with lock_path.open("a+", encoding="utf-8") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        if manifest_path.exists():
            manifest = _read_manifest(manifest_path)
            if manifest.get("task", {}).get("slug") != args.task:
                raise ValueError("manifest task slug does not match --task")
            if args.base_sha and manifest.get("task", {}).get("base_sha") != args.base_sha:
                raise ValueError("manifest base SHA does not match --base-sha")
        else:
            manifest = _empty_manifest(args.task, args.base_sha, cwd, args.started_at)

        if metadata:
            _merge_metadata(manifest, metadata)

        if any(run.get("id") == args.run_id for run in manifest.get("runs", [])):
            raise ValueError(f"duplicate run id: {args.run_id}")

        artifacts = [_parse_artifact(value, cwd, args.run_id, index) for index, value in enumerate(args.artifact)]
        result = "PASS" if args.exit_code == 0 else "FAIL"
        input_sha256 = hashlib.sha256(args.inputs.encode("utf-8")).hexdigest()
        command_sha256 = hashlib.sha256(
            json.dumps(args.command, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        run = {
            "id": args.run_id,
            "category": args.category,
            "stem": args.stem,
            "inputs": args.inputs,
            "cwd": str(cwd),
            "command": args.command,
            "command_display": shlex.join(args.command),
            "started_at": args.started_at,
            "ended_at": args.ended_at,
            "elapsed_seconds": args.elapsed_seconds,
            "exit_code": args.exit_code,
            "result": result,
            "log": _absolute(args.log, cwd),
            "status_file": _absolute(args.status_file, cwd),
            "artifacts": artifacts,
            "input_sha256": input_sha256,
            "command_sha256": command_sha256,
            "base_sha": manifest.get("task", {}).get("base_sha", args.base_sha),
        }
        for artifact in artifacts:
            artifact.update(
                base_sha=run["base_sha"],
                input_sha256=input_sha256,
                command_sha256=command_sha256,
            )
        manifest.setdefault("runs", []).append(run)
        manifest.setdefault("artifacts", []).extend(artifacts)
        _atomic_write(manifest_path, manifest)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--metadata")
    parser.add_argument("--task", required=True)
    parser.add_argument("--base-sha", default="")
    parser.add_argument("--cwd", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--category", required=True)
    parser.add_argument("--stem", required=True)
    parser.add_argument("--inputs", required=True)
    parser.add_argument("--started-at", required=True)
    parser.add_argument("--ended-at", required=True)
    parser.add_argument("--elapsed-seconds", type=float, required=True)
    parser.add_argument("--exit-code", type=int, required=True)
    parser.add_argument("--log", required=True)
    parser.add_argument("--status-file", required=True)
    parser.add_argument("--artifact", action="append", default=[])
    parser.add_argument("--command-json", dest="command", type=_parse_command_json, required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    append_run(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
