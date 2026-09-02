#!/usr/bin/env python3
"""Download production feedback, reproduction bundles and failed solve runs.

Production server: root@114.66.55.78 (ssh key ~/.ssh/id_rsa).

Run (one command, all three stages):
    python3 scripts/sync_feedback.py

Stages:
1. Import new feedback bundles into feedback/<date>/<time>-<note>/.
2. Import the matching remote cli-runs reproduction bundle for each diagnosticId.
3. Capture failed solve runs (serve responses with ok=false) that have no user
   feedback into evidence/<date>-<error-class>/raw-runs/, deduplicated by run id.

Optional overrides:
    python3 scripts/sync_feedback.py --target root@1.2.3.4 --remote-dir /var/lib/arknights-infra/feedback
    python3 scripts/sync_feedback.py --no-failed-runs
    python3 scripts/sync_feedback.py --evidence-dir /tmp/evidence --failed-run-hours 12
    SSH_PASSWORD=... python3 scripts/sync_feedback.py --target arkinfra-server
"""

from __future__ import annotations

import argparse
import hashlib
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
DEFAULT_LOCAL_EVIDENCE_DIR = ROOT / "evidence"
DEFAULT_NOTE = "feedback"
DEFAULT_FAILED_RUN_HOURS = 72

CAPTURE_MANIFEST_HEADER = (
    "run_dir\trotation\ttop\toperbox_sha256_16\tlayout_sha256_16\tprod_ok\tprod_error"
)

FAILURE_SLUG_RULES = [
    ("belongs to multiple teams", "belongs-to-multiple-teams"),
    ("all peak prefix candidates are infeasible", "aic-plan-3004"),
]

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


def remote_failed_runs(target: str, cli_runs_dir: str, password: str) -> dict[str, str]:
    quoted = shlex.quote(cli_runs_dir)
    script = (
        f"for path in {quoted}/*; do "
        'test -f "$path/serve-request.jsonl" || continue; '
        'test -f "$path/serve-response.json" || continue; '
        'ok=$(jq -r \'.ok\' "$path/serve-response.json"); '
        'test "$ok" = "false" || continue; '
        'message=$(jq -r \'.error.message // empty\' "$path/serve-response.json"); '
        'test -n "$message" || continue; '
        'printf \'%s\\t%s\\n\' "$(basename "$path")" "$message"; '
        "done"
    )
    cmd = with_password_tool(["ssh", target, script], password)
    try:
        result = subprocess.run(
            cmd,
            env=command_env(password),
            capture_output=True,
            text=True,
            timeout=120,
        )
    except subprocess.TimeoutExpired:
        print(f"Skip timed out failed-run scan: {cli_runs_dir}", flush=True)
        return {}
    if result.returncode != 0:
        print(f"Skip unavailable failed-run scan: {cli_runs_dir}", flush=True)
        return {}

    failed: dict[str, str] = {}
    for line in result.stdout.splitlines():
        name, _, message = line.partition("\t")
        if name and message:
            failed[name] = message
    return failed


def run_timestamp(run_name: str) -> datetime | None:
    prefix = run_name.split("_", 1)[0]
    for fmt in ("%Y-%m-%dT%H-%M-%S-%fZ", "%Y-%m-%dT%H-%M-%SZ"):
        try:
            parsed = datetime.strptime(prefix, fmt)
        except ValueError:
            continue
        return parsed.replace(tzinfo=timezone.utc).astimezone(LOCAL_TZ)
    return None


def failure_slug(message: str) -> str:
    lowered = message.lower()
    for needle, slug in FAILURE_SLUG_RULES:
        if needle in lowered:
            return slug
    tokens = re.findall(r"[a-z0-9]+", lowered)
    if tokens:
        return "-".join(tokens[:6])[:60] or "unclassified"
    return "unclassified"


def existing_captured_run_names(evidence_root: Path) -> set[str]:
    if not evidence_root.exists():
        return set()
    names: set[str] = set()
    for raw_root in evidence_root.glob("*/raw-runs"):
        if raw_root.is_dir():
            names.update(path.name for path in raw_root.iterdir() if path.is_dir())
    return names


def manifest_rows(group_dir: Path) -> list[str]:
    raw_root = group_dir / "raw-runs"
    rows: list[str] = []
    if not raw_root.is_dir():
        return rows
    for run_dir in sorted(raw_root.iterdir()):
        if not run_dir.is_dir():
            continue
        name = run_dir.name
        request = json_file_data(run_dir / "serve-request.json")
        params = request.get("params", {}) if isinstance(request, dict) else {}
        options = params.get("options", {}) if isinstance(params, dict) else {}
        rotation = options.get("rotation") or params.get("rotation") or "?"
        top = options.get("top") or params.get("top") or "?"
        operbox_sha = run_dir / "operbox.json"
        layout_sha = run_dir / "layout.json"
        response = json_file_data(run_dir / "serve-response.json")
        ok = response.get("ok") if isinstance(response, dict) else None
        error = response.get("error", {}) if isinstance(response, dict) else {}
        message = error.get("message", "") if isinstance(error, dict) else ""
        rows.append(
            "\t".join(
                [
                    name,
                    str(rotation).replace("\t", " "),
                    str(top).replace("\t", " "),
                    sha256_prefix(operbox_sha),
                    sha256_prefix(layout_sha),
                    "false" if ok is False else ("true" if ok is True else "?"),
                    str(message).replace("\t", " ").replace("\n", " "),
                ]
            )
        )
    return rows


def sha256_prefix(path: Path) -> str:
    if not path.is_file():
        return "-"
    try:
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
    except OSError:
        return "-"
    return digest[:16]


def write_manifest(group_dir: Path) -> None:
    (group_dir / "manifest.tsv").write_text(
        CAPTURE_MANIFEST_HEADER + "\n" + "\n".join(manifest_rows(group_dir)) + "\n",
        encoding="utf-8",
    )


def write_evidence_readme(group_dir: Path, first_run: str, message: str) -> None:
    text = f"""# 求解失败自动回放包

捕获来源：生产 cli-runs 中无用户反馈的失败 `serve` 运行（自动同步，{datetime.now(LOCAL_TZ):%Y-%m-%d %H:%M}）。

- 首个失败运行：{first_run}
- 错误原文：{message}

目录：`raw-runs/` 为原样失败运行（serve-request.jsonl / serve-response.json / layout / operbox / debug-bundle / 日志）。`manifest.tsv` 为输入摘要。本目录含真实练度盒，保持私有，不提交公共仓库。

复现：

```bash
cd <solver-repo-root>
cargo build --release -p infra-cli
./{group_dir.relative_to(ROOT)}/replay.sh <solver-repo-root>/target/release/infra-cli <outdir>
```
"""
    (group_dir / "README.md").write_text(text, encoding="utf-8")


def write_evidence_replay_script(group_dir: Path) -> None:
    script = f"""#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")" && pwd)"
cli=${{1:?usage: replay.sh INFRA_CLI [OUT_DIR]}}
out=${{2:-"$root/local-replay"}}

test -x "$cli"
mkdir -p "$out"

failures=0
total=0
for dir in "$root"/raw-runs/*/; do
  [ -f "$dir/serve-request.jsonl" ] || continue
  total=$((total + 1))
  name="$(basename "$dir")"
  response="$out/$name.jsonl"
  timeout --kill-after=15s 600s "$cli" serve < "$dir/serve-request.jsonl" > "$response" 2> "$out/$name.stderr.log"
  ok=$(jq -r '.ok' "$response")
  message=$(jq -r '.error.message // ""' "$response")
  printf '%s ok=%s error=%s\\n' "$name" "$ok" "$message"
  if [ "$ok" = "false" ]; then
    :
  else
    printf '  UNEXPECTED: expected ok=false\\n' >&2
    failures=$((failures + 1))
  fi
done

printf 'replays=%s unexpected=%s out=%s\\n' "$total" "$failures" "$out"
test "$failures" -eq 0
"""
    path = group_dir / "replay.sh"
    path.write_text(script, encoding="utf-8")
    path.chmod(0o755)


def capture_failed_runs(
    evidence_root: Path,
    target: str,
    remote_feedback_dir: str,
    temp_dir: Path,
    password: str,
    max_age_hours: int,
) -> list[tuple[Path, int]]:
    remote_dir = remote_run_dir(remote_feedback_dir)
    if not remote_dir_exists(target, remote_dir, password):
        return []

    failed = remote_failed_runs(target, remote_dir, password)
    if not failed:
        return []

    now = datetime.now(timezone.utc)
    known_names = existing_captured_run_names(evidence_root)
    groups: dict[Path, list[str]] = {}
    for name, message in sorted(failed.items()):
        stamp = run_timestamp(name)
        if stamp is None:
            print(f"Skip failed run with unparsable time: {name}", flush=True)
            continue
        if (now - stamp.astimezone(timezone.utc)) > timedelta(hours=max_age_hours):
            continue
        if name in known_names:
            continue
        day = stamp.strftime("%Y-%m-%d")
        slug = failure_slug(message)
        group_dir = evidence_root / f"{day}-{slug}"
        created = not group_dir.exists()
        group_dir.mkdir(parents=True, exist_ok=True)
        if created:
            write_evidence_readme(group_dir, name, message)
        if not (group_dir / "replay.sh").exists():
            write_evidence_replay_script(group_dir)
        groups.setdefault(group_dir, []).append(name)

    for group_dir in groups:
        remote_paths = {
            name: f"{remote_dir}/{name}" for name in sorted(groups[group_dir])
        }
        downloaded = download_remote_runs(
            target, remote_paths, temp_dir / "captured-runs", password
        )
        for name, source_dir in downloaded.items():
            target_dir = group_dir / "raw-runs" / name
            target_dir.parent.mkdir(parents=True, exist_ok=True)
            if target_dir.exists():
                continue
            shutil.copytree(source_dir, target_dir)
            print(f"Captured failed run: {group_dir.relative_to(ROOT)}/raw-runs/{name}", flush=True)

    captured: list[tuple[Path, int]] = []
    for group_dir in sorted(groups):
        if manifest_managed(group_dir):
            write_manifest(group_dir)
            captured.append((group_dir, len(groups[group_dir])))
        else:
            print(
                f"Skip manifest refresh for unmanaged evidence dir: {group_dir.relative_to(ROOT)}",
                flush=True,
            )

    for name, message in failed.items():
        stamp = run_timestamp(name)
        if stamp is None:
            continue
        group_dir = evidence_root / f"{stamp.strftime('%Y-%m-%d')}-{failure_slug(message)}"
        if group_dir in groups or not (group_dir / "raw-runs").is_dir():
            continue
        if manifest_managed(group_dir):
            write_manifest(group_dir)

    return captured


def manifest_managed(group_dir: Path) -> bool:
    manifest = group_dir / "manifest.tsv"
    if not manifest.is_file():
        write_manifest(group_dir)
        return True
    try:
        header = manifest.read_text(encoding="utf-8").splitlines()[0]
    except (OSError, IndexError):
        return False
    return header == CAPTURE_MANIFEST_HEADER


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
    parser.add_argument(
        "--evidence-dir",
        type=Path,
        default=DEFAULT_LOCAL_EVIDENCE_DIR,
        help="local evidence directory for captured failed runs",
    )
    parser.add_argument(
        "--no-failed-runs",
        action="store_true",
        help="skip the failed solve run capture stage",
    )
    parser.add_argument(
        "--failed-run-hours",
        type=int,
        default=DEFAULT_FAILED_RUN_HOURS,
        help="capture failed runs no older than this many hours",
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
    evidence_dir = resolve_local_dir(args.evidence_dir)
    remote_dirs = args.remote_dir or DEFAULT_REMOTE_FEEDBACK_DIRS

    print(f"SSH target: {args.target}", flush=True)
    for remote_dir in remote_dirs:
        print(f"Remote feedback: {remote_dir}", flush=True)
    print(f"Local feedback: {local_feedback_dir}", flush=True)
    print(f"Local evidence: {evidence_dir}", flush=True)

    with tempfile.TemporaryDirectory(prefix="feedback-sync-") as temp:
        known = existing_feedback_by_id(local_feedback_dir)
        organized: list[Path] = []
        reproduced: list[Path] = []
        captured: list[tuple[Path, int]] = []

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
            if not args.no_failed_runs:
                captured.extend(
                    capture_failed_runs(
                        evidence_dir,
                        args.target,
                        remote_dir,
                        Path(temp),
                        password,
                        args.failed_run_hours,
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

    print("\nFailed solve runs captured into evidence:")
    if not captured:
        print("(none)")
    for group_dir, count in captured:
        print(f"{display_path(group_dir)}: +{count} run(s)")

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
