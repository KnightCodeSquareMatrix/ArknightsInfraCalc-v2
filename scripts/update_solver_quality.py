#!/usr/bin/env python3
"""Push the local Linux infra-cli to the fixed remote quality-check service.

Implements docs/QUALITY_CHECK_CLI_UPDATE.md (embeddedData hot-path, no bake):

    cargo build --release -p infra-cli
    python3 scripts/update_solver_quality.py

The script intentionally has no configurable deployment mode. It uploads the
fixed release ELF to the fixed remote incoming directory, atomically renames
it, then polls the fixed remote status API until the quality job is terminal.
"""

from __future__ import annotations

import hashlib
import json
import shlex
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL_ELF = ROOT / "target" / "release" / "infra-cli"
REMOTE_TARGET = "root@114.66.55.78"
REMOTE_INCOMING = "/var/lib/arknights-infra-quality/solver-incoming"
STATUS_URL = "https://instance-pi2ohhfj.tail2dca9.ts.net:10000/api/quality/status"
POLL_SEC = 5
TIMEOUT_SEC = 1800


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


def main() -> int:
    if len(sys.argv) != 1:
        raise SystemExit("usage: python3 scripts/update_solver_quality.py")

    local_elf = LOCAL_ELF
    if not local_elf.is_file():
        raise SystemExit(
            f"local ELF does not exist: {local_elf}\n"
            "Build it first with: cargo build --release -p infra-cli"
        )
    ensure_elf(local_elf)
    executable_sha256 = sha256(local_elf)

    print(f"Local ELF: {local_elf}")
    print(f"Local sha256: {executable_sha256}")
    print(f"Incoming dir: {REMOTE_INCOMING}")
    print(f"Status URL: {STATUS_URL}")
    print(f"SSH target: {REMOTE_TARGET}")

    partial = f"{REMOTE_INCOMING}/infra-cli.partial"
    final = f"{REMOTE_INCOMING}/infra-cli"
    run(["scp", str(local_elf), f"{REMOTE_TARGET}:{partial}"])
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
    run(["ssh", REMOTE_TARGET, "bash", "-s"], stdin=remote_script)

    print(f"\nPolling {STATUS_URL} ...")
    deadline = time.monotonic() + TIMEOUT_SEC
    bundle_id: str | None = None
    job_id: str | None = None
    terminal = {"completed", "failed", "cancelled", "paused"}

    while time.monotonic() < deadline:
        try:
            data = fetch_json(STATUS_URL)
        except Exception as exc:  # noqa: BLE001
            print(f"  status fetch error: {exc}")
            time.sleep(POLL_SEC)
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

        time.sleep(POLL_SEC)

    raise SystemExit(
        f"timed out after {TIMEOUT_SEC:.0f}s; "
        f"bundle={bundle_id} job={job_id} not terminal"
    )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode) from exc
