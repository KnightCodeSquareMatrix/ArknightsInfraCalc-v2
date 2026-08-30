#!/usr/bin/env python3
"""Capture a traceable Fiammetta rotation baseline matrix.

The generated packet is immutable by default and keeps source, binary, input,
command, raw output, parsed rotation, MAA output, and compact summaries linked
by SHA-256 digests.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import csv
import datetime as dt
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BOX_DIR = ROOT / "data/fixtures/collected_operboxes/collected_operboxes"
DEFAULT_OUTPUT_ROOT = ROOT / "target/codex-runs/fiammetta-baseline"
DEFAULT_BINARY = ROOT / "target/release/infra-cli"
DEFAULT_LAYOUTS = ("153", "243", "333")
DEFAULT_ROTATIONS = ("3", "abc-121212")
FIAMMETTA_MODES = (("enabled", False), ("disabled", True))


class CaptureError(RuntimeError):
    pass


@dataclass(frozen=True)
class CaseSpec:
    case_id: str
    layout: str
    layout_path: Path
    rotation_arg: str
    rotation: str
    fiammetta: str
    no_fiammetta: bool
    box_id: str
    box_path: Path


@dataclass
class CaseResult:
    case_id: str
    layout: str
    rotation: str
    fiammetta: str
    box_id: str
    status: str
    failure_category: str
    exit_code: int | None
    elapsed_seconds: float
    timed_out: bool
    command: list[str]
    case_dir: str
    stdout: str
    stderr: str
    rotation_report: str | None
    maa: str | None
    error: str | None
    artifact_sha256: dict[str, str]
    daily_trade: float | None = None
    daily_manufacture: float | None = None
    daily_power: float | None = None
    fiammetta_targets: list[str] | None = None
    trade_rooms_by_shift: list[dict[str, Any]] | None = None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-root", type=Path, default=DEFAULT_OUTPUT_ROOT)
    parser.add_argument("--run-id", help="Unique output directory name")
    parser.add_argument("--box-dir", type=Path, default=DEFAULT_BOX_DIR)
    parser.add_argument("--box-ids", nargs="+", help="Optional Box IDs for a smoke subset")
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--layouts", nargs="+", default=list(DEFAULT_LAYOUTS))
    parser.add_argument("--rotations", nargs="+", default=list(DEFAULT_ROTATIONS))
    parser.add_argument("--top", type=int, default=20)
    parser.add_argument("--jobs", type=int, default=min(4, os.cpu_count() or 1))
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--build", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--release", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--assert-invariants", action="store_true")
    parser.add_argument("--fail-on-case-error", action="store_true")
    args = parser.parse_args()
    if args.top < 1:
        parser.error("--top must be positive")
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    if args.timeout < 1:
        parser.error("--timeout must be positive")
    return args


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def generated_run_id() -> str:
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    head = git_text("rev-parse", "--short=12", "HEAD") or "no-git"
    return f"{stamp}-{head}"


def git_text(*args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=False
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_new_bytes(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(payload)


def write_new_text(path: Path, text: str) -> None:
    write_new_bytes(path, text.encode("utf-8"))


def write_new_json(path: Path, value: Any) -> None:
    write_new_text(path, json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def run_capture(command: list[str], *, timeout: int | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def build_binary(args: argparse.Namespace, run_dir: Path) -> dict[str, Any]:
    log_path = run_dir / "build/build.log"
    if not args.build:
        if not args.binary.is_file():
            raise CaptureError(f"binary not found: {args.binary}")
        return {"requested": False, "exit_code": None, "log": None}
    profile = "release" if args.release else "debug"
    command = ["cargo", "build", "--locked"]
    if args.release:
        command.append("--release")
    command += ["-p", "infra-cli"]
    started_at = utc_now()
    started = time.monotonic()
    result = run_capture(command)
    elapsed = time.monotonic() - started
    log = (
        f"cwd={ROOT}\ncommand={shlex.join(command)}\nstarted_at={started_at}\n"
        f"elapsed_seconds={elapsed:.6f}\nexit_code={result.returncode}\n"
        "--- stdout ---\n"
        f"{result.stdout}\n--- stderr ---\n{result.stderr}"
    )
    write_new_text(log_path, log)
    expected = ROOT / f"target/{profile}/infra-cli"
    if result.returncode != 0:
        raise CaptureError(f"cargo build failed; see {log_path}")
    if args.binary.resolve() != expected.resolve():
        raise CaptureError(
            f"--build produced {expected}, but --binary points to {args.binary}; "
            "use --no-build for an external binary"
        )
    return {
        "requested": True,
        "command": command,
        "exit_code": result.returncode,
        "elapsed_seconds": round(elapsed, 6),
        "log": log_path.relative_to(run_dir).as_posix(),
        "log_sha256": sha256_file(log_path),
    }


def copy_input(path: Path, destination: Path) -> dict[str, Any]:
    data = path.read_bytes()
    write_new_bytes(destination, data)
    return {
        "source": display_path(path),
        "snapshot": destination,
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
    }


def snapshot_inputs(
    run_dir: Path, layouts: dict[str, Path], boxes: list[Path]
) -> tuple[dict[str, Any], dict[str, Path], list[Path]]:
    snapshots: dict[str, Any] = {"layouts": {}, "boxes": {}}
    frozen_layouts: dict[str, Path] = {}
    frozen_boxes: list[Path] = []
    for name, path in layouts.items():
        destination = run_dir / "inputs/layouts" / path.name
        record = copy_input(path, destination)
        record["snapshot"] = destination.relative_to(run_dir).as_posix()
        snapshots["layouts"][name] = record
        frozen_layouts[name] = destination
    for path in boxes:
        destination = run_dir / "inputs/boxes" / path.name
        record = copy_input(path, destination)
        record["snapshot"] = destination.relative_to(run_dir).as_posix()
        snapshots["boxes"][box_id_from_path(path)] = record
        frozen_boxes.append(destination)
    return snapshots, frozen_layouts, frozen_boxes


def git_bytes(*args: str) -> bytes:
    result = subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, check=False
    )
    if result.returncode != 0:
        raise CaptureError(
            f"git {' '.join(args)} failed: {result.stderr.decode('utf-8', errors='replace')}"
        )
    return result.stdout


def source_inventory() -> dict[str, Any]:
    paths = [
        value.decode("utf-8", errors="surrogateescape")
        for value in git_bytes(
            "ls-files", "--cached", "--others", "--exclude-standard", "-z"
        ).split(b"\0")
        if value
    ]
    entries = []
    for relative in sorted(paths):
        path = ROOT / relative
        if path.is_symlink():
            target = os.readlink(path)
            payload = target.encode("utf-8", errors="surrogateescape")
            entries.append(
                {
                    "path": relative,
                    "kind": "symlink",
                    "bytes": len(payload),
                    "sha256": hashlib.sha256(payload).hexdigest(),
                    "target": target,
                }
            )
        elif path.is_file():
            entries.append(
                {
                    "path": relative,
                    "kind": "file",
                    "bytes": path.stat().st_size,
                    "sha256": sha256_file(path),
                }
            )
        else:
            entries.append(
                {"path": relative, "kind": "missing", "bytes": 0, "sha256": None}
            )
    canonical = json.dumps(
        entries, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return {
        "schema_version": 1,
        "head": git_text("rev-parse", "HEAD"),
        "branch": git_text("branch", "--show-current"),
        "created_at_utc": utc_now(),
        "inventory_sha256": hashlib.sha256(canonical).hexdigest(),
        "entry_count": len(entries),
        "entries": entries,
    }


def capture_source_state(run_dir: Path, label: str) -> dict[str, Any]:
    inventory = source_inventory()
    write_new_json(run_dir / f"source/inventory-{label}.json", inventory)
    if label == "before-build":
        write_new_bytes(run_dir / "source/worktree.patch", git_bytes("diff", "--binary", "HEAD"))
        write_new_bytes(
            run_dir / "source/status-porcelain-v1.bin",
            git_bytes("status", "--porcelain=v1", "-z", "--untracked-files=all"),
        )
        write_new_text(run_dir / "source/status-short.txt", git_text("status", "--short") + "\n")
        script_snapshot = run_dir / "source/capture-script.py"
        write_new_bytes(script_snapshot, Path(__file__).read_bytes())
    return inventory


def box_id_from_path(path: Path) -> str:
    suffix = "-operbox"
    return path.stem[: -len(suffix)] if path.stem.endswith(suffix) else path.stem


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return str(path.resolve())


def rotation_label(argument: str) -> str:
    aliases = {
        "3": "abc_12_6_6",
        "abc": "abc_12_6_6",
        "12/6/6": "abc_12_6_6",
        "abc-1266": "abc_12_6_6",
        "abc-121212": "abc_12_12_12",
        "12/12/12": "abc_12_12_12",
        "abc-12-12-12": "abc_12_12_12",
    }
    return aliases.get(argument, argument.replace("/", "_"))


def enumerate_cases(
    layouts: dict[str, Path], rotations: Iterable[str], boxes: list[Path]
) -> list[CaseSpec]:
    cases = []
    for rotation_arg in rotations:
        rotation = rotation_label(rotation_arg)
        for layout, layout_path in layouts.items():
            for fiammetta, no_fiammetta in FIAMMETTA_MODES:
                for box_path in boxes:
                    box_id = box_id_from_path(box_path)
                    case_id = f"{rotation}__{layout}__{fiammetta}__{box_id}"
                    cases.append(
                        CaseSpec(
                            case_id=case_id,
                            layout=layout,
                            layout_path=layout_path,
                            rotation_arg=rotation_arg,
                            rotation=rotation,
                            fiammetta=fiammetta,
                            no_fiammetta=no_fiammetta,
                            box_id=box_id,
                            box_path=box_path,
                        )
                    )
    return cases


def artifact_digest(path: Path | None) -> str | None:
    return sha256_file(path) if path is not None and path.is_file() else None


def classify_failure(stderr: str, timed_out: bool) -> str:
    if timed_out:
        return "timeout"
    lowered = stderr.lower()
    if "assert-invariants" in lowered:
        return "invariant"
    if "operbox parse" in lowered or "invalid type" in lowered or "not in blueprint" in lowered:
        return "input"
    return "solver"


def last_error(stderr: str, stdout: str) -> str:
    lines = [line.strip() for line in stderr.splitlines() if line.strip()]
    if not lines:
        lines = [line.strip() for line in stdout.splitlines() if line.strip()]
    return lines[-1] if lines else "command failed without output"


def summarize_report(report: dict[str, Any]) -> dict[str, Any]:
    daily = report.get("daily") or {}
    targets = []
    trade_rooms_by_shift = []
    for shift in report.get("shifts") or []:
        action = shift.get("fiammetta")
        if isinstance(action, dict) and isinstance(action.get("target"), str):
            targets.append(action["target"])
        rooms = []
        assignment = shift.get("assignment") or {}
        for room in assignment.get("rooms") or []:
            room_id = room.get("room_id")
            if isinstance(room_id, dict):
                room_id = room_id.get("0") or room_id.get("value")
            if not isinstance(room_id, str) or not room_id.startswith("trade_"):
                continue
            rooms.append(
                {
                    "room_id": room_id,
                    "operators": [
                        operator.get("name")
                        for operator in room.get("operators") or []
                        if isinstance(operator, dict) and isinstance(operator.get("name"), str)
                    ],
                }
            )
        rooms.sort(key=lambda room: room["room_id"])
        trade_rooms_by_shift.append(
            {"shift": shift.get("index", len(trade_rooms_by_shift)) + 1, "rooms": rooms}
        )
    return {
        "daily_trade": daily.get("trade"),
        "daily_manufacture": daily.get("manufacture"),
        "daily_power": daily.get("power"),
        "fiammetta_targets": targets,
        "trade_rooms_by_shift": trade_rooms_by_shift,
    }


def run_case(
    case: CaseSpec,
    *,
    run_dir: Path,
    binary: Path,
    top: int,
    timeout: int,
    assert_invariants: bool,
) -> CaseResult:
    case_dir = run_dir / "cases" / case.case_id
    case_dir.mkdir(parents=True, exist_ok=False)
    rotation_path = case_dir / "rotation.json"
    maa_path = case_dir / "maa.json"
    profile_path = case_dir / "profile.json"
    stdout_path = case_dir / "stdout.log"
    stderr_path = case_dir / "stderr.log"
    status_path = case_dir / "status.json"
    command_path = case_dir / "command.json"
    command = [
        str(binary),
        "plan",
        "--layout",
        str(case.layout_path),
        "--operbox",
        str(case.box_path),
        "--rotation",
        case.rotation_arg,
        "--top",
        str(top),
        "--rotation-out",
        str(rotation_path),
        "--profile-out",
        str(profile_path),
        "--maa-out",
        str(maa_path),
        "--json",
    ]
    if case.no_fiammetta:
        command.append("--no-fiammetta")
    if assert_invariants:
        command.append("--assert-invariants")
    write_new_json(
        command_path,
        {
            "cwd": str(ROOT),
            "argv": command,
            "shell_display": shlex.join(command),
            "layout_sha256": sha256_file(case.layout_path),
            "operbox_sha256": sha256_file(case.box_path),
            "binary_sha256": sha256_file(binary),
        },
    )
    started_at = utc_now()
    started = time.monotonic()
    timed_out = False
    exit_code: int | None
    try:
        result = run_capture(command, timeout=timeout)
        stdout = result.stdout
        stderr = result.stderr
        exit_code = result.returncode
    except subprocess.TimeoutExpired as error:
        timed_out = True
        exit_code = None
        stdout = error.stdout or ""
        stderr = error.stderr or ""
        if isinstance(stdout, bytes):
            stdout = stdout.decode("utf-8", errors="replace")
        if isinstance(stderr, bytes):
            stderr = stderr.decode("utf-8", errors="replace")
        stderr += f"\ntimeout after {timeout}s\n"
    elapsed = time.monotonic() - started
    write_new_text(stdout_path, stdout)
    write_new_text(stderr_path, stderr)

    report: dict[str, Any] | None = None
    parse_error = None
    if rotation_path.is_file():
        try:
            report = json.loads(rotation_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            parse_error = f"rotation report parse failed: {error}"
    success = (
        exit_code == 0
        and report is not None
        and maa_path.is_file()
        and profile_path.is_file()
    )
    failure_category = "" if success else classify_failure(stderr, timed_out)
    error = None if success else (parse_error or last_error(stderr, stdout))
    summary = summarize_report(report) if report is not None else {}
    artifact_paths = {
        "command": command_path,
        "stdout": stdout_path,
        "stderr": stderr_path,
        "rotation_report": rotation_path if rotation_path.is_file() else None,
        "maa": maa_path if maa_path.is_file() else None,
        "profile": profile_path if profile_path.is_file() else None,
    }
    digests = {
        name: digest
        for name, path in artifact_paths.items()
        if (digest := artifact_digest(path)) is not None
    }
    status = {
        "schema_version": 1,
        "case": asdict(case) | {
            "layout_path": case.layout_path.relative_to(ROOT).as_posix(),
            "box_path": case.box_path.relative_to(ROOT).as_posix(),
        },
        "started_at_utc": started_at,
        "ended_at_utc": utc_now(),
        "elapsed_seconds": round(elapsed, 6),
        "exit_code": exit_code,
        "timed_out": timed_out,
        "status": "completed" if success else "failed",
        "failure_category": failure_category,
        "error": error,
        "artifacts": {
            name: {
                "path": path.relative_to(run_dir).as_posix(),
                "sha256": digests[name],
            }
            for name, path in artifact_paths.items()
            if path is not None and name in digests
        },
        "summary": summary,
    }
    write_new_json(status_path, status)
    digests["status"] = sha256_file(status_path)
    return CaseResult(
        case_id=case.case_id,
        layout=case.layout,
        rotation=case.rotation,
        fiammetta=case.fiammetta,
        box_id=case.box_id,
        status=status["status"],
        failure_category=failure_category,
        exit_code=exit_code,
        elapsed_seconds=round(elapsed, 6),
        timed_out=timed_out,
        command=command,
        case_dir=case_dir.relative_to(run_dir).as_posix(),
        stdout=stdout_path.relative_to(run_dir).as_posix(),
        stderr=stderr_path.relative_to(run_dir).as_posix(),
        rotation_report=(
            rotation_path.relative_to(run_dir).as_posix() if rotation_path.is_file() else None
        ),
        maa=maa_path.relative_to(run_dir).as_posix() if maa_path.is_file() else None,
        error=error,
        artifact_sha256=digests,
        **summary,
    )


def write_summary(run_dir: Path, results: list[CaseResult]) -> None:
    summary_dir = run_dir / "summary"
    summary_dir.mkdir(parents=True, exist_ok=False)
    results.sort(key=lambda result: result.case_id)
    write_new_json(summary_dir / "cases.json", [asdict(result) for result in results])
    csv_path = summary_dir / "cases.csv"
    csv_path.parent.mkdir(parents=True, exist_ok=True)
    with csv_path.open("x", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(
            [
                "case_id",
                "layout",
                "rotation",
                "fiammetta",
                "box_id",
                "status",
                "failure_category",
                "exit_code",
                "elapsed_seconds",
                "daily_trade",
                "daily_manufacture",
                "daily_power",
                "fiammetta_targets",
                "case_dir",
                "error",
            ]
        )
        for result in results:
            writer.writerow(
                [
                    result.case_id,
                    result.layout,
                    result.rotation,
                    result.fiammetta,
                    result.box_id,
                    result.status,
                    result.failure_category,
                    result.exit_code,
                    result.elapsed_seconds,
                    result.daily_trade,
                    result.daily_manufacture,
                    result.daily_power,
                    "|".join(result.fiammetta_targets or []),
                    result.case_dir,
                    result.error or "",
                ]
            )
    comparison_rows = []
    by_key = {
        (result.rotation, result.layout, result.box_id, result.fiammetta): result
        for result in results
    }
    for rotation, layout, box_id in sorted(
        {(result.rotation, result.layout, result.box_id) for result in results}
    ):
        enabled = by_key.get((rotation, layout, box_id, "enabled"))
        disabled = by_key.get((rotation, layout, box_id, "disabled"))
        comparable = bool(
            enabled
            and disabled
            and enabled.status == "completed"
            and disabled.status == "completed"
        )
        comparison_rows.append(
            {
                "rotation": rotation,
                "layout": layout,
                "box_id": box_id,
                "comparable": comparable,
                "enabled_case": enabled.case_id if enabled else None,
                "disabled_case": disabled.case_id if disabled else None,
                "enabled_trade": enabled.daily_trade if enabled else None,
                "disabled_trade": disabled.daily_trade if disabled else None,
                "trade_delta": (
                    enabled.daily_trade - disabled.daily_trade
                    if comparable
                    and enabled.daily_trade is not None
                    and disabled.daily_trade is not None
                    else None
                ),
                "enabled_manufacture": enabled.daily_manufacture if enabled else None,
                "disabled_manufacture": disabled.daily_manufacture if disabled else None,
                "manufacture_delta": (
                    enabled.daily_manufacture - disabled.daily_manufacture
                    if comparable
                    and enabled.daily_manufacture is not None
                    and disabled.daily_manufacture is not None
                    else None
                ),
                "enabled_power": enabled.daily_power if enabled else None,
                "disabled_power": disabled.daily_power if disabled else None,
                "power_delta": (
                    enabled.daily_power - disabled.daily_power
                    if comparable
                    and enabled.daily_power is not None
                    and disabled.daily_power is not None
                    else None
                ),
                "enabled_targets": enabled.fiammetta_targets if enabled else None,
                "trade_rooms_changed": (
                    enabled.trade_rooms_by_shift != disabled.trade_rooms_by_shift
                    if comparable
                    else None
                ),
                "enabled_error": enabled.error if enabled else "missing enabled case",
                "disabled_error": disabled.error if disabled else "missing disabled case",
            }
        )
    write_new_json(summary_dir / "comparisons.json", comparison_rows)
    with (summary_dir / "comparisons.csv").open("x", encoding="utf-8", newline="") as stream:
        fieldnames = [
            "rotation",
            "layout",
            "box_id",
            "comparable",
            "enabled_case",
            "disabled_case",
            "enabled_trade",
            "disabled_trade",
            "trade_delta",
            "enabled_manufacture",
            "disabled_manufacture",
            "manufacture_delta",
            "enabled_power",
            "disabled_power",
            "power_delta",
            "enabled_targets",
            "trade_rooms_changed",
            "enabled_error",
            "disabled_error",
        ]
        writer = csv.DictWriter(stream, fieldnames=fieldnames)
        writer.writeheader()
        for row in comparison_rows:
            serialized = row.copy()
            serialized["enabled_targets"] = "|".join(row["enabled_targets"] or [])
            writer.writerow(serialized)
    failures = [result for result in results if result.status != "completed"]
    counts: dict[str, int] = {}
    for result in failures:
        counts[result.failure_category] = counts.get(result.failure_category, 0) + 1
    write_new_json(
        summary_dir / "overview.json",
        {
            "schema_version": 1,
            "case_count": len(results),
            "completed": len(results) - len(failures),
            "failed": len(failures),
            "failure_categories": counts,
            "comparison_count": len(comparison_rows),
            "comparable": sum(bool(row["comparable"]) for row in comparison_rows),
            "trade_room_changes": sum(
                row["trade_rooms_changed"] is True for row in comparison_rows
            ),
            "fiammetta_target_cases": sum(
                bool(result.fiammetta_targets) for result in results
            ),
        },
    )


def packet_files(run_dir: Path) -> list[dict[str, Any]]:
    records = []
    for path in sorted(run_dir.rglob("*")):
        if not path.is_file() or path.name == "packet-manifest.json":
            continue
        records.append(
            {
                "path": path.relative_to(run_dir).as_posix(),
                "bytes": path.stat().st_size,
                "sha256": sha256_file(path),
            }
        )
    return records


def main() -> int:
    args = parse_args()
    output_root = args.output_root.resolve()
    run_id = args.run_id or generated_run_id()
    if not re.fullmatch(r"[A-Za-z0-9._-]+", run_id):
        raise CaptureError("--run-id accepts only A-Z, a-z, 0-9, dot, underscore, and dash")
    run_dir = output_root / run_id
    if run_dir.exists():
        raise CaptureError(f"refusing to overwrite existing run directory: {run_dir}")
    run_dir.mkdir(parents=True, exist_ok=False)
    started_at = utc_now()
    try:
        layouts = {
            name: (ROOT / "data/layout" / f"{name}.json").resolve()
            for name in args.layouts
        }
        missing_layouts = [str(path) for path in layouts.values() if not path.is_file()]
        if missing_layouts:
            raise CaptureError(f"missing layouts: {missing_layouts}")
        boxes = sorted(
            path.resolve()
            for path in args.box_dir.glob("*-operbox.json")
            if not path.stem.endswith("_profile")
        )
        if args.box_ids:
            requested = set(args.box_ids)
            boxes = [path for path in boxes if box_id_from_path(path) in requested]
            missing = sorted(requested - {box_id_from_path(path) for path in boxes})
            if missing:
                raise CaptureError(f"requested Box IDs not found: {missing}")
        if not boxes:
            raise CaptureError(f"no *-operbox.json inputs in {args.box_dir}")
        source_before = capture_source_state(run_dir, "before-build")
        inputs, frozen_layouts, frozen_boxes = snapshot_inputs(run_dir, layouts, boxes)
        cases = enumerate_cases(frozen_layouts, args.rotations, frozen_boxes)
        expected = (
            len(frozen_layouts)
            * len(args.rotations)
            * len(frozen_boxes)
            * len(FIAMMETTA_MODES)
        )
        if len(cases) != expected or len({case.case_id for case in cases}) != expected:
            raise CaptureError("matrix case IDs are not complete and unique")
        build = build_binary(args, run_dir)
        source_after_build = capture_source_state(run_dir, "after-build")
        if source_before["inventory_sha256"] != source_after_build["inventory_sha256"]:
            raise CaptureError("source worktree changed while building; refusing mixed provenance")
        built_binary = args.binary.resolve()
        if not built_binary.is_file():
            raise CaptureError(f"binary not found after build: {built_binary}")
        binary = run_dir / "binary/infra-cli"
        write_new_bytes(binary, built_binary.read_bytes())
        binary.chmod(0o755)
        provenance = {
            "schema_version": 1,
            "run_id": run_id,
            "started_at_utc": started_at,
            "repository": str(ROOT),
            "git": {
                "head": git_text("rev-parse", "HEAD"),
                "branch": git_text("branch", "--show-current"),
                "status_short": git_text("status", "--short"),
            },
            "binary": {
                "source_path": display_path(built_binary),
                "snapshot": binary.relative_to(run_dir).as_posix(),
                "sha256": sha256_file(binary),
                "bytes": binary.stat().st_size,
            },
            "source": {
                "before_build_inventory": "source/inventory-before-build.json",
                "after_build_inventory": "source/inventory-after-build.json",
                "inventory_sha256": source_before["inventory_sha256"],
                "stable_during_build": True,
                "worktree_patch": "source/worktree.patch",
                "status_short": "source/status-short.txt",
                "capture_script": "source/capture-script.py",
            },
            "build": build,
            "matrix": {
                "layouts": list(layouts),
                "rotations": [rotation_label(value) for value in args.rotations],
                "rotation_args": args.rotations,
                "fiammetta_modes": [mode for mode, _ in FIAMMETTA_MODES],
                "boxes": [box_id_from_path(path) for path in boxes],
                "case_count": len(cases),
                "top": args.top,
                "timeout_seconds": args.timeout,
                "jobs": args.jobs,
                "assert_invariants": args.assert_invariants,
            },
            "inputs": inputs,
        }
        write_new_json(run_dir / "provenance.json", provenance)
        write_new_json(
            run_dir / "matrix.json",
            [
                asdict(case)
                | {
                    "layout_path": case.layout_path.relative_to(run_dir).as_posix(),
                    "box_path": case.box_path.relative_to(run_dir).as_posix(),
                }
                for case in cases
            ],
        )
        results = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as executor:
            future_by_case = {
                executor.submit(
                    run_case,
                    case,
                    run_dir=run_dir,
                    binary=binary,
                    top=args.top,
                    timeout=args.timeout,
                    assert_invariants=args.assert_invariants,
                ): case
                for case in cases
            }
            completed = 0
            for future in concurrent.futures.as_completed(future_by_case):
                case = future_by_case[future]
                try:
                    result = future.result()
                except Exception as error:  # preserve the rest of the matrix
                    result = CaseResult(
                        case_id=case.case_id,
                        layout=case.layout,
                        rotation=case.rotation,
                        fiammetta=case.fiammetta,
                        box_id=case.box_id,
                        status="failed",
                        failure_category="harness",
                        exit_code=None,
                        elapsed_seconds=0.0,
                        timed_out=False,
                        command=[],
                        case_dir=f"cases/{case.case_id}",
                        stdout="",
                        stderr="",
                        rotation_report=None,
                        maa=None,
                        error=f"harness error: {error}",
                        artifact_sha256={},
                    )
                results.append(result)
                completed += 1
                print(
                    f"[{completed}/{len(cases)}] {result.status.upper()} "
                    f"{result.case_id} {result.elapsed_seconds:.2f}s",
                    flush=True,
                )
        write_summary(run_dir, results)
        source_after_run = capture_source_state(run_dir, "after-run")
        source_stable_after_build = (
            source_after_build["inventory_sha256"]
            == source_after_run["inventory_sha256"]
        )
        overview = json.loads(
            (run_dir / "summary/overview.json").read_text(encoding="utf-8")
        )
        packet = {
            "schema_version": 1,
            "run_id": run_id,
            "created_at_utc": utc_now(),
            "provenance": "provenance.json",
            "matrix": "matrix.json",
            "overview": "summary/overview.json",
            "case_index": "summary/cases.json",
            "comparison_index": "summary/comparisons.json",
            "source_stable_after_build": source_stable_after_build,
            "files": packet_files(run_dir),
        }
        write_new_json(run_dir / "packet-manifest.json", packet)
        print(
            f"BASELINE_PACKET={run_dir}\n"
            f"cases={overview['case_count']} completed={overview['completed']} "
            f"failed={overview['failed']} comparable={overview['comparable']}",
            flush=True,
        )
        return 1 if args.fail_on_case_error and overview["failed"] else 0
    except (CaptureError, OSError, subprocess.SubprocessError) as error:
        failure_path = run_dir / "capture-error.json"
        if not failure_path.exists():
            write_new_json(
                failure_path,
                {
                    "schema_version": 1,
                    "run_id": run_id,
                    "started_at_utc": started_at,
                    "failed_at_utc": utc_now(),
                    "error": str(error),
                },
            )
        print(f"error: {error}\npartial_packet={run_dir}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
