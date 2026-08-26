#!/usr/bin/env python3
"""Push the local Linux infra-cli to the quality-check service.

Implements docs/QUALITY_CHECK_CLI_UPDATE.md (embeddedData hot-path, no bake):

    cargo build --release -p infra-cli
    python3 scripts/update_solver_quality.py

The script uploads the ELF to solver-incoming/infra-cli.partial, atomically
renames it, then polls the public quality status API until the new bundle and
its job appear and the job reaches a terminal state.

Optional overrides:

    python3 scripts/update_solver_quality.py --target root@1.2.3.4
    python3 scripts/update_solver_quality.py --local-elf /some/path/infra-cli
    python3 scripts/update_solver_quality.py --status-url https://.../api/quality/status
    python3 scripts/update_solver_quality.py --poll-sec 5 --timeout-sec 1800

Defaults match the quality-check environment on 114.66.55.78.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shlex
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

DEFAULT_SSH_TARGET = "root@114.66.55.78"
DEFAULT_LOCAL_ELF = ROOT / "target" / "release" / "infra-cli"
DEFAULT_INCOMING = "/var/lib/arknights-infra-quality/solver-incoming"
DEFAULT_STATUS_URL = "https://instance-pi2ohhfj.tail2dca9.ts.net:10000/api/quality/status"
DEFAULT_POLL_SEC = 5
DEFAULT_TIMEOUT_SEC = 1800


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def ensure_elf(path: Path) -> None:
    with path.open("rb") as f:
        magic = f.read(4)
    if magic != b"\x7fELF":
        raise SystemExit(f"{path} is not a Linux ELF. Do not upload infra-cli.exe.")


def run(cmd: list[str], *, stdin: str | None = None) -> None:
    print(f"\n$ {shlex.join(cmd)}")
    subprocess.run(cmd, input=stdin, text=True, check=True)


def fetch_json(url: str, timeout: float = 15) -> dict:
    with urllib.request.urlopen(url, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def find_new_bundle(data: dict, executable_sha256: str) -> dict | None:
    bundles = data.get("data", {}).get("bundles", [])
    if not bundles:
        bundles = data.get("data", {}).get("bundle", [])
    for bundle in bundles:
        if bundle.get("executableSha256") == executable_sha256:
            return bundle
    return None


def find_job_for_bundle(data: dict, bundle_id: str) -> dict | None:
    jobs = data.get("data", {}).get("jobs", [])
    for job in jobs:
        if job.get("bundleId") == bundle_id:
            return job
    return None


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=DEFAULT_SSH_TARGET, help="ssh target host")
    parser.add_argument(
        "--local-elf",
        type=Path,
        default=DEFAULT_LOCAL_ELF,
        help="local Linux infra-cli ELF to upload",
    )
    parser.add_argument(
        "--incoming",
        default=DEFAULT_INCOMING,
        help="remote quality solver-incoming dir",
    )
    parser.add_argument(
        "--status-url",
        default=DEFAULT_STATUS_URL,
        help="quality status API URL",
    )
    parser.add_argument(
        "--poll-sec",
        type=float,
        default=DEFAULT_POLL_SEC,
        help="poll interval seconds",
    )
    parser.add_argument(
        "--timeout-sec",
        type=float,
        default=DEFAULT_TIMEOUT_SEC,
        help="max seconds to wait for the job to finish",
    )
    parser.add_argument(
        "--no-wait",
        action="store_true",
        help="upload only; do not poll the status API",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)

    local_elf = args.local_elf.expanduser().resolve()
    if not local_elf.is_file():
        raise SystemExit(
            f"local ELF does not exist: {local_elf}\n"
            "Build it first with: cargo build --release -p infra-cli"
        )
    ensure_elf(local_elf)
    executable_sha256 = sha256(local_elf)

    print(f"Local ELF: {local_elf}")
    print(f"Local sha256: {executable_sha256}")
    print(f"SSH target: {args.target}")
    print(f"Incoming dir: {args.incoming}")

    partial = f"{args.incoming}/infra-cli.partial"
    final = f"{args.incoming}/infra-cli"

    run(["scp", str(local_elf), f"{args.target}:{partial}"])

    remote_script = (
        "set -euo pipefail\n"
        f"partial={shlex.quote(partial)}\n"
        f"final={shlex.quote(final)}\n"
        "test -f \"$partial\"\n"
        "chown arkinfra:arkinfra \"$partial\"\n"
        "chmod 750 \"$partial\"\n"
        f"sha256sum \"$partial\"\n"
        f"mv -f \"$partial\" \"$final\"\n"
        "ls -la \"$(dirname \"$final\")\"\n"
    )
    run(["ssh", args.target, "bash", "-s"], stdin=remote_script)

    if args.no_wait:
        print("\nUploaded. Not waiting (--no-wait).")
        return 0

    print(f"\nPolling {args.status_url} ...")
    deadline = time.monotonic() + args.timeout_sec
    bundle_id: str | None = None
    job_id: str | None = None
    terminal = {"completed", "failed", "cancelled", "paused"}

    while time.monotonic() < deadline:
        try:
            data = fetch_json(args.status_url)
        except Exception as exc:  # noqa: BLE001
            print(f"  status fetch error: {exc}")
            time.sleep(args.poll_sec)
            continue

        if bundle_id is None:
            bundle = find_new_bundle(data, executable_sha256)
            if bundle:
                bundle_id = bundle.get("bundleId")
                print(f"  bundle found: {bundle_id}")
                print(f"    gitCommit={bundle.get('gitCommit')} "
                      f"autoDiscovered={bundle.get('autoDiscovered')}")

        if bundle_id is not None and job_id is None:
            job = find_job_for_bundle(data, bundle_id)
            if job:
                job_id = job.get("jobId")
                print(f"  job found: {job_id} status={job.get('status')}")

        if job_id is not None:
            job = find_job_for_bundle(data, bundle_id or "")
            if job and job.get("status") in terminal:
                print(f"  job terminal: status={job.get('status')}")
                print(f"    completed={job.get('completedCount')} "
                      f"failed={job.get('failedCount')} "
                      f"interrupted={job.get('interruptedCount')} "
                      f"/ {job.get('caseCount')}")
                print(f"    caseStatuses={job.get('caseStatuses')}")
                return 0 if job.get("status") == "completed" else 2

        time.sleep(args.poll_sec)

    raise SystemExit(
        f"timed out after {args.timeout_sec:.0f}s; "
        f"bundle={bundle_id} job={job_id} not terminal"
    )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode) from exc
