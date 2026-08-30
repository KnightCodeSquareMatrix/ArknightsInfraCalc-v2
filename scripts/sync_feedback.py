#!/usr/bin/env python3
"""Download production feedback and reproduction bundles for local debugging.

Production server: root@114.66.55.78 (ssh key ~/.ssh/id_rsa).

Run:
    python3 scripts/sync_feedback.py

Optional overrides:
    python3 scripts/sync_feedback.py --target root@1.2.3.4 --remote-dir /var/lib/arknights-infra/feedback
    SSH_PASSWORD=... python3 scripts/sync_feedback.py --target arkinfra-server
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone, timedelta
from pathlib import Path
from pathlib import PurePosixPath


ROOT = Path(__file__).resolve().parents[1]

DEFAULT_SSH_TARGET = "root@114.66.55.78"
DEFAULT_REMOTE_FEEDBACK_DIRS = [
    "/var/lib/arknights-infra/feedback",
    "/var/lib/arknights-infra-dev/feedback",
    "/var/lib/arknights-infra-quality/feedback",
]
DEFAULT_LOCAL_FEEDBACK_DIR = ROOT / "feedback"
DEFAULT_NOTE = "feedback"

# These files make a run bundle usable for both the safe debug-rerun adapter
# and direct inspection of the original online request and result.
REQUIRED_REPRODUCTION_ARTIFACTS = (
    "debug-bundle.json",
    "operbox.json",
    "layout.json",
    "command.txt",
    "serve-request.jsonl",
    "result.json",
)

# Local feedback folders use Beijing time (CST, UTC+8) for day + time prefix.
LOCAL_TZ = timezone(timedelta(hours=8))


def with_password_tool(base_cmd: list[str], password: str) -> list[str]:
    if not password:
        return base_cmd

    if shutil.which("sshpass") is None:
        raise SystemExit(
            "SSH_PASSWORD is set, but sshpass was not found. "
            "Install sshpass or unset SSH_PASSWORD and use ssh normally."
        )

    return ["sshpass", "-e", *base_cmd]


def run(cmd: list[str], *, password: str) -> None:
    print(f"\n$ {shlex.join(cmd)}", flush=True)
    subprocess.run(cmd, env=command_env(password), check=True)


def command_env(password: str) -> dict[str, str]:
    env = os.environ.copy()
    if password:
        env["SSHPASS"] = password
    return env


def copy_feedback(source: Path, target: Path) -> None:
    if source.is_dir():
        shutil.copytree(source, target, dirs_exist_ok=True)
        return

    target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target / source.name)


def find_issue_json(item: Path) -> Path | None:
    if item.is_file() and item.name == "issue.json":
        return item
    if not item.is_dir():
        return None

    direct = item / "issue.json"
    if direct.is_file():
        return direct

    matches = sorted(item.rglob("issue.json"))
    return matches[0] if matches else None


def issue_data(item: Path) -> dict:
    issue = find_issue_json(item)
    if issue is None:
        return {}

    try:
        data = json.loads(issue.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}

    return data if isinstance(data, dict) else {}


def json_file_data(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return data if isinstance(data, dict) else {}


def find_meta_json(item: Path) -> Path | None:
    if item.is_file() and item.name == "meta.json":
        return item
    if not item.is_dir():
        return None

    direct = item / "meta.json"
    if direct.is_file():
        return direct

    matches = sorted(item.rglob("meta.json"))
    return matches[0] if matches else None


def feedback_id(item: Path) -> str | None:
    meta = find_meta_json(item)
    if meta is None:
        return None
    value = json_file_data(meta).get("feedbackId")
    if isinstance(value, str) and value.strip():
        return value.strip()
    return None


def existing_feedback_by_id(root: Path) -> dict[str, Path]:
    by_id: dict[str, Path] = {}
    if not root.exists():
        return by_id

    for meta in root.glob("*/*/meta.json"):
        value = json_file_data(meta).get("feedbackId")
        if isinstance(value, str) and value.strip():
            by_id[value.strip()] = meta.parent
    return by_id


def first_string(data: dict, keys: list[str]) -> str | None:
    for key in keys:
        value = data.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def first_time(data: dict) -> datetime | None:
    for key in ["time", "timestamp", "createdAt", "created_at", "submittedAt", "date"]:
        value = data.get(key)
        if isinstance(value, (int, float)):
            if value > 10_000_000_000:
                value = value / 1000
            return datetime.fromtimestamp(value, tz=timezone.utc)
        if isinstance(value, str) and value.strip():
            parsed = parse_time(value.strip())
            if parsed is not None:
                return parsed
    return None


def parse_time(value: str) -> datetime | None:
    normalized = value
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"

    for parser in [
        lambda text: datetime.fromisoformat(text),
        lambda text: datetime.strptime(text, "%Y-%m-%d %H:%M:%S").replace(tzinfo=LOCAL_TZ),
        lambda text: datetime.strptime(text, "%Y-%m-%d_%H-%M-%S").replace(tzinfo=LOCAL_TZ),
        lambda text: datetime.strptime(text, "%Y%m%d%H%M%S").replace(tzinfo=LOCAL_TZ),
    ]:
        try:
            return parser(normalized)
        except ValueError:
            pass
    return None


def meta_saved_time(item: Path) -> datetime | None:
    meta = find_meta_json(item)
    if meta is None:
        return None
    return first_time(json_file_data(meta))


def item_time(item: Path, data: dict) -> datetime:
    parsed = first_time(data)
    if parsed is None:
        parsed = meta_saved_time(item)
    if parsed is None:
        parsed = datetime.fromtimestamp(item.stat().st_mtime, tz=timezone.utc)
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=LOCAL_TZ)
    return parsed.astimezone(LOCAL_TZ)


def item_note(item: Path, data: dict) -> str:
    note = first_string(
        data,
        [
            "note",
            "title",
            "summary",
            "message",
            "description",
            "problem",
            "bug",
        ],
    )
    if note is None:
        note = item.stem
    return slug(note) or DEFAULT_NOTE


def slug(value: str, limit: int = 80) -> str:
    value = value.strip().lower()
    value = re.sub(r"\s+", "-", value)
    value = re.sub(r"[^0-9a-zA-Z._\-\u4e00-\u9fff]+", "-", value)
    value = re.sub(r"-+", "-", value).strip("-._")
    return value[:limit].strip("-._")


def organize_feedback(raw_root: Path, local_feedback_dir: Path, known: dict[str, Path] | None = None) -> list[Path]:
    local_feedback_dir.mkdir(parents=True, exist_ok=True)
    if known is None:
        known = existing_feedback_by_id(local_feedback_dir)
    organized: list[Path] = []
    items = sorted(
        [path for path in raw_root.iterdir() if path.name not in {".", ".."}],
        key=lambda path: path.name,
    )

    for item in items:
        data = issue_data(item)
        when = item_time(item, data)
        note = item_note(item, data)
        day_dir = local_feedback_dir / when.strftime("%Y-%m-%d")
        day_dir.mkdir(parents=True, exist_ok=True)
        folder_name = f"{when.strftime('%H%M%S')}-{note}"
        fid = feedback_id(item)
        target = known.get(fid) if fid is not None else None
        is_new = target is None
        if target is None:
            target = day_dir / folder_name
        if fid is not None:
            known[fid] = target
        copy_feedback(item, target)
        if is_new:
            organized.append(target)

    return organized


def remote_dir_exists(target: str, remote_dir: str, password: str) -> bool:
    command = f"test -d {shlex.quote(remote_dir)} && echo 1"
    cmd = with_password_tool(["ssh", target, command], password)
    try:
        probe = subprocess.run(
            cmd,
            env=command_env(password),
            capture_output=True,
            text=True,
            timeout=20,
        )
    except subprocess.TimeoutExpired:
        return False
    return probe.returncode == 0 and "1" in probe.stdout


def download_remote_dir(target: str, remote_dir: str, local_dir: Path, password: str) -> None:
    local_dir.mkdir(parents=True, exist_ok=True)
    if shutil.which("rsync") is not None:
        cmd = with_password_tool(
            [
                "rsync",
                "-az",
                f"{target}:{remote_dir}/",
                f"{local_dir}/",
            ],
            password,
        )
    else:
        cmd = with_password_tool(
            [
                "scp",
                "-r",
                f"{target}:{remote_dir}/.",
                str(local_dir),
            ],
            password,
        )
    run(cmd, password=password)


def diagnostic_id(item: Path) -> str | None:
    meta = find_meta_json(item)
    if meta is None:
        return None
    value = json_file_data(meta).get("diagnosticId")
    if isinstance(value, str) and value.strip():
        return value.strip()
    return None


def reproduction_artifacts_ready(case_dir: Path) -> bool:
    return all((case_dir / name).is_file() for name in REQUIRED_REPRODUCTION_ARTIFACTS)


def remote_run_dir(remote_feedback_dir: str) -> str:
    return str(PurePosixPath(remote_feedback_dir).parent / "cli-runs")


def remote_run_paths(
    target: str, remote_dir: str, diagnostic_ids: set[str], password: str
) -> dict[str, str]:
    if not diagnostic_ids:
        return {}

    quoted_dir = shlex.quote(remote_dir)
    command = (
        f"for path in {quoted_dir}/*; do "
        "test -d \"$path\" && printf '%s\\n' \"$path\"; "
        "done"
    )
    cmd = with_password_tool(["ssh", target, command], password)
    try:
        result = subprocess.run(
            cmd,
            env=command_env(password),
            capture_output=True,
            text=True,
            timeout=60,
        )
    except subprocess.TimeoutExpired:
        print(f"Skip timed out remote run directory: {remote_dir}", flush=True)
        return {}
    if result.returncode != 0:
        print(f"Skip unavailable remote run directory: {remote_dir}", flush=True)
        return {}

    matches: dict[str, str] = {}
    for value in result.stdout.splitlines():
        path = value.strip()
        if not path:
            continue
        name = PurePosixPath(path).name
        for diagnostic in diagnostic_ids:
            if name.endswith(f"_{diagnostic}"):
                matches.setdefault(diagnostic, path)
    return matches


def download_remote_runs(
    target: str,
    remote_paths: dict[str, str],
    local_root: Path,
    password: str,
) -> dict[str, Path]:
    if not remote_paths:
        return {}

    local_root.mkdir(parents=True, exist_ok=True)
    if shutil.which("rsync") is not None:
        filters: list[str] = []
        for remote_path in remote_paths.values():
            run_name = PurePosixPath(remote_path).name
            filters.extend(["--include", f"{run_name}/***"])
        cmd = with_password_tool(
            [
                "rsync",
                "-az",
                *filters,
                "--exclude",
                "*",
                f"{target}:{PurePosixPath(next(iter(remote_paths.values()))).parent}/",
                f"{local_root}/",
            ],
            password,
        )
        run(cmd, password=password)
    else:
        for remote_path in remote_paths.values():
            run_name = PurePosixPath(remote_path).name
            download_remote_dir(target, remote_path, local_root / run_name, password)
    return {
        diagnostic: local_root / PurePosixPath(remote_path).name
        for diagnostic, remote_path in remote_paths.items()
    }


def incomplete_artifacts(run_dir: Path) -> list[str]:
    return [name for name in REQUIRED_REPRODUCTION_ARTIFACTS if not (run_dir / name).is_file()]


def sync_reproduction_artifacts(
    raw_root: Path,
    known: dict[str, Path],
    target: str,
    remote_feedback_dir: str,
    temp_dir: Path,
    password: str,
) -> list[Path]:
    targets_by_diagnostic: dict[str, list[Path]] = {}
    for item in sorted(raw_root.iterdir()):
        feedback = feedback_id(item)
        diagnostic = diagnostic_id(item)
        if feedback is None or diagnostic is None:
            continue
        case_dir = known.get(feedback)
        if case_dir is None or reproduction_artifacts_ready(case_dir):
            continue
        targets_by_diagnostic.setdefault(diagnostic, []).append(case_dir)

    remote_paths = remote_run_paths(
        target,
        remote_run_dir(remote_feedback_dir),
        set(targets_by_diagnostic),
        password,
    )
    for diagnostic in targets_by_diagnostic:
        if diagnostic not in remote_paths:
            print(f"No run bundle for diagnosticId: {diagnostic}", flush=True)

    local_run_dirs = download_remote_runs(
        target,
        remote_paths,
        temp_dir / "run-bundles",
        password,
    )
    imported: list[Path] = []
    for diagnostic, case_dirs in targets_by_diagnostic.items():
        source_dir = local_run_dirs.get(diagnostic)
        if source_dir is None:
            continue
        missing = incomplete_artifacts(source_dir)
        if missing:
            print(
                f"Incomplete run bundle for {diagnostic}: {', '.join(missing)}",
                flush=True,
            )
            continue

        for case_dir in case_dirs:
            for artifact in source_dir.iterdir():
                if artifact.is_file():
                    shutil.copy2(artifact, case_dir / artifact.name)
            imported.append(case_dir)

    return imported


def recent_files(root: Path, limit: int = 20) -> list[Path]:
    if not root.exists():
        return []
    files = [path for path in root.rglob("*") if path.is_file()]
    files.sort(key=lambda path: path.stat().st_mtime, reverse=True)
    return files[:limit]


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=DEFAULT_SSH_TARGET, help="ssh target alias/host")
    parser.add_argument(
        "--remote-dir",
        action="append",
        default=[],
        help="remote feedback directory (repeatable; defaults to all three services)",
    )
    parser.add_argument(
        "--local-dir",
        type=Path,
        default=DEFAULT_LOCAL_FEEDBACK_DIR,
        help="local feedback directory",
    )
    return parser.parse_args(argv)


def resolve_local_dir(path: Path) -> Path:
    expanded = path.expanduser()
    if expanded.is_absolute():
        return expanded
    return ROOT / expanded


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        return str(path)


def print_connection_hint(target: str) -> None:
    print(
        "\nConnection hint:\n"
        f"- SSH target `{target}` could not be reached by rsync/scp.\n"
        "- If it is an SSH alias, add it to ~/.ssh/config, for example:\n"
        "  Host arkinfra-server\n"
        "    HostName <server-ip-or-domain>\n"
        "    User root\n"
        "- Or pass the real target directly:\n"
        "  python3 scripts/sync_feedback.py --target root@<server-ip-or-domain>\n"
        "- If the host requires a password, run with SSH_PASSWORD=... and ensure sshpass is installed.",
        flush=True,
    )


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    password = os.environ.get("SSH_PASSWORD", "")
    local_feedback_dir = resolve_local_dir(args.local_dir)
    remote_dirs = args.remote_dir or DEFAULT_REMOTE_FEEDBACK_DIRS

    print(f"SSH target: {args.target}", flush=True)
    for remote_dir in remote_dirs:
        print(f"Remote feedback: {remote_dir}", flush=True)
    print(f"Local feedback: {local_feedback_dir}", flush=True)

    with tempfile.TemporaryDirectory(prefix="feedback-sync-") as temp:
        known = existing_feedback_by_id(local_feedback_dir)
        organized: list[Path] = []
        reproduced: list[Path] = []

        for remote_dir in remote_dirs:
            raw_root = Path(temp) / re.sub(r"[^0-9a-zA-Z._-]+", "_", remote_dir.strip("/").replace("/", "_"))
            if not remote_dir_exists(args.target, remote_dir, password):
                print(f"Skip missing remote dir: {remote_dir}", flush=True)
                continue

            try:
                download_remote_dir(args.target, remote_dir, raw_root, password)
            except subprocess.CalledProcessError as exc:
                if exc.returncode == 255:
                    print_connection_hint(args.target)
                raise
            organized.extend(organize_feedback(raw_root, local_feedback_dir, known))
            reproduced.extend(
                sync_reproduction_artifacts(
                    raw_root,
                    known,
                    args.target,
                    remote_dir,
                    Path(temp),
                    password,
                )
            )

    print("\nNew feedback folders synced:")
    if not organized:
        print("(none)")
    for path in organized:
        print(display_path(path))

    print("\nReproduction bundles imported:")
    if not reproduced:
        print("(none)")
    for path in reproduced:
        print(display_path(path))

    files = recent_files(local_feedback_dir)
    print("\nRecent feedback files:")
    if not files:
        print("(none)")
    for path in files:
        print(display_path(path))

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode) from exc
