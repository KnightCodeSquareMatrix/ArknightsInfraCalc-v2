#!/usr/bin/env python3
"""Build and atomically update the production infra-cli solver.

This script intentionally fixes the production target. It delegates file
replacement and rollback to update_solver_dev.py, then verifies the internal
service and public HTTPS health endpoint.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from urllib.request import urlopen

from update_solver_dev import main as update_solver
from update_solver_dev import sha256

ROOT = Path(__file__).resolve().parents[1]
LOCAL_ELF = ROOT / "target" / "release" / "infra-cli"
SSH_TARGET = "root@114.66.55.78"
APP_ROOT = "/opt/arknights-infra"
SERVICE = "arknights-infra"
INTERNAL_PORT = 4175
PUBLIC_HEALTH_URL = "https://instance-pi2ohhfj.tail2dca9.ts.net:8443/api/health"


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print(f"\n$ {' '.join(command)}")
    subprocess.run(command, check=True, env=env)


def current_commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()


def built_at() -> str:
    return subprocess.run(
        ["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"],
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()


def assert_health(payload: object, source: str) -> None:
    if not isinstance(payload, dict):
        raise RuntimeError(f"{source} health response is not an object")
    data = payload.get("data")
    if not isinstance(data, dict):
        raise RuntimeError(f"{source} health response has no data object")
    features = data.get("features")
    if (
        payload.get("success") is not True
        or data.get("status") != "ready"
        or data.get("plannerReady") is not True
        or not isinstance(features, dict)
        or set(features) != {"debugTools", "rateLimit"}
        or features.get("debugTools") is not False
        or features.get("rateLimit") is not True
    ):
        raise RuntimeError(f"{source} production health assertion failed: {payload}")


def verify_remote_install(expected_sha256: str) -> None:
    remote_script = f"""set -euo pipefail
expected_sha256={expected_sha256}
app=$(readlink -f -- {APP_ROOT}/current)
installed_cli="$app/bin/infra-cli"
env_file="$app/.env.production.local"
test -f "$installed_cli"
test ! -L "$installed_cli"
test -f "$env_file"
test ! -L "$env_file"
test "$(sha256sum "$installed_cli" | cut -d ' ' -f 1)" = "$expected_sha256"
grep -Fx "INFRA_CLI_EXPECTED_SHA256=$expected_sha256" "$env_file"
systemctl is-active --quiet {SERVICE}
printf '%s\n' '{{"id":"production-update","method":"ping","params":{{}}}}' \
  | runuser -u arkinfra -- "$installed_cli" serve \
  | node -e '
    let input = "";
    process.stdin.on("data", chunk => input += chunk);
    process.stdin.on("end", () => {{
      const body = JSON.parse(input);
      const result = body?.result;
      const expected = process.argv[1];
      if (
        body?.ok !== true ||
        result?.pong !== true ||
        result?.protocol_version !== 1 ||
        !Array.isArray(result?.supported_plan_schema_versions) ||
        !result.supported_plan_schema_versions.includes(1) ||
        result?.solver_executable_sha256 !== expected
      ) process.exit(1);
    }});
  ' "$expected_sha256"
curl -fsS http://127.0.0.1:{INTERNAL_PORT}/api/health | node -e '
  let input = "";
  process.stdin.on("data", chunk => input += chunk);
  process.stdin.on("end", () => {{
    const body = JSON.parse(input);
    const features = body?.data?.features;
    if (
      body?.success !== true ||
      body?.data?.status !== "ready" ||
      body?.data?.plannerReady !== true ||
      !features ||
      Object.keys(features).length !== 2 ||
      features?.debugTools !== false ||
      features?.rateLimit !== true
    ) process.exit(1);
  }});
'
"""
    subprocess.run(["ssh", SSH_TARGET, "bash", "-s"], input=remote_script, text=True, check=True)


def verify_public_health() -> None:
    with urlopen(PUBLIC_HEALTH_URL, timeout=30) as response:
        payload = json.load(response)
    assert_health(payload, "public")
    print("Public production health: ready")


def main() -> int:
    if len(sys.argv) != 1:
        raise SystemExit("usage: python3 scripts/update_solver_production.py")

    commit = current_commit()
    timestamp = built_at()
    print("[1/4 BUILD] compiling the current Linux solver")
    print(f"  git_commit={commit}")
    print(f"  built_at={timestamp}")
    run(
        ["cargo", "build", "--locked", "--offline", "--release", "-p", "infra-cli"],
        env={**os.environ, "SOLVER_GIT_COMMIT": commit, "SOLVER_BUILT_AT": timestamp},
    )

    expected_sha256 = sha256(LOCAL_ELF)
    print("[2/4 ARTIFACT] Linux ELF verified")
    print(f"  path={LOCAL_ELF}")
    print(f"  sha256={expected_sha256}")

    print("[3/4 INSTALL] atomically updating production")
    update_solver(
        [
            "--target",
            SSH_TARGET,
            "--local-elf",
            str(LOCAL_ELF),
            "--app-root",
            APP_ROOT,
            "--service",
            SERVICE,
            "--port",
            str(INTERNAL_PORT),
        ]
    )

    print("[4/4 VERIFY] checking installed fingerprint and public health")
    verify_remote_install(expected_sha256)
    verify_public_health()
    print(f"[RESULT] production solver updated: {expected_sha256}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode) from exc
