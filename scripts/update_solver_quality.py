#!/usr/bin/env python3
"""Build and push infra-cli to the remote Quality runner.

Implements docs/QUALITY_CHECK_CLI_UPDATE.md (embeddedData hot-path, no bake):

    python3 scripts/update_solver_quality.py

The runner only installs and archives the ELF. Job creation is intentionally
left to an explicit, separate Quality API request.
"""

from __future__ import annotations

import hashlib
import argparse
import os
import shlex
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL_ELF = ROOT / "target" / "release" / "infra-cli"
REMOTE_TARGET = "root@114.66.55.78"
REMOTE_INCOMING = "/var/lib/arknights-infra-quality/solver-incoming"


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


def run(cmd: list[str], *, stdin: str | None = None, env: dict[str, str] | None = None) -> None:
    print(f"\n$ {shlex.join(cmd)}")
    subprocess.run(cmd, input=stdin, text=True, check=True, env=env)


def checkpoint(name: str, detail: str) -> None:
    print(f"\n[{name}] {detail}")


def workspace_status() -> tuple[str, bool]:
    result = subprocess.run(
        ["git", "status", "--short", "--branch"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
    )
    lines = result.stdout.strip().splitlines()
    branch = lines[0] if lines else "unknown branch"
    dirty = len(lines) > 1
    return branch, dirty


def current_commit() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
    )
    return result.stdout.strip()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh-target", default=REMOTE_TARGET)
    args = parser.parse_args()
    remote_target = args.ssh_target

    checkpoint("1/5 WORKSPACE", f"repository={ROOT}")
    branch, dirty = workspace_status()
    print(f"  {branch}")
    print(f"  state={'DIRTY (building current worktree)' if dirty else 'CLEAN'}")

    checkpoint("2/5 BUILD", "cargo build --release -p infra-cli")
    solver_git_commit = current_commit()
    solver_built_at = subprocess.run(
        ["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"],
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()
    print(f"  git_commit={solver_git_commit}")
    print(f"  built_at={solver_built_at}")
    run(
        ["cargo", "build", "--release", "-p", "infra-cli"],
        env={**os.environ, "SOLVER_GIT_COMMIT": solver_git_commit, "SOLVER_BUILT_AT": solver_built_at},
    )

    local_elf = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))) / "release" / "infra-cli"
    ensure_elf(local_elf)
    executable_sha256 = sha256(local_elf)

    checkpoint("3/5 ARTIFACT", "Linux ELF verified")
    print(f"  path={local_elf}")
    print(f"  sha256={executable_sha256}")

    checkpoint("4/5 PUSH", f"target={remote_target} incoming={REMOTE_INCOMING}")

    partial = f"{REMOTE_INCOMING}/infra-cli.{executable_sha256}.partial"
    final = f"{REMOTE_INCOMING}/infra-cli"
    run(["scp", str(local_elf), f"{remote_target}:{partial}"])
    remote_script = (
        "set -euo pipefail\n"
        f"partial={shlex.quote(partial)}\n"
        f"final={shlex.quote(final)}\n"
        f"lock={shlex.quote(REMOTE_INCOMING + '/.infra-cli-upload.lock')}\n"
        "for attempt in $(seq 1 30); do\n"
        "  if mkdir \"$lock\" 2>/dev/null; then break; fi\n"
        "  if [ \"$attempt\" -eq 30 ]; then exit 75; fi\n"
        "  sleep 1\n"
        "done\n"
        "trap 'rmdir \"$lock\"' EXIT\n"
        "test -f \"$partial\"\n"
        "test ! -e \"$final\"\n"
        "chown arkinfra:arkinfra \"$partial\"\n"
        "chmod 750 \"$partial\"\n"
        f"sha256sum \"$partial\"\n"
        f"mv \"$partial\" \"$final\"\n"
        "ls -la \"$(dirname \"$final\")\"\n"
    )
    run(["ssh", remote_target, "bash", "-s"], stdin=remote_script)

    checkpoint("5/5 HANDOFF", "ELF atomically handed to the runner; no job created")
    print("\n[RESULT] HANDED OFF: runner installation is asynchronous")
    print(f"  sha256={executable_sha256}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode) from exc
