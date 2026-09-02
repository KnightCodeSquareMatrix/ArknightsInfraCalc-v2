#!/usr/bin/env python3
"""Build and deploy the Linux infra-cli to the live website server.

This is the single deployment entry for the shared-solver architecture:

    python3 scripts/update_solver_production.py

The script fixes the production target by default. It performs, in order:

1. Build the current worktree with SOLVER_GIT_COMMIT / SOLVER_BUILT_AT
   injected, after verifying that crates/ has no uncommitted changes (the
   artifact must be uniquely identified by the tagged commit).
2. Smoke-test the local ELF over `serve` (protocol 1, schema support,
   executable fingerprint).
3. Update the GitHub Environment variable DEPLOY_APPROVED_SOLVER_SHA256.
4. Upload the ELF, atomically replace the root-owned shared/bin/infra-cli
   and its single-line SHA-256 sidecar on the server (old pair is backed up).
5. Derive a release archive from the active release (helper-owned paths
   excluded) and activate it through the server deployment helper
   (/usr/local/sbin/arknights-infra-deploy), which re-injects the solver
   into the new release, restarts website and plan worker, and rolls back
   automatically on failure.

Optional overrides:

    python3 scripts/update_solver_production.py --environment development
    python3 scripts/update_solver_production.py --local-elf /some/path/infra-cli
    python3 scripts/update_solver_production.py --ssh-target root@1.2.3.4
    python3 scripts/update_solver_production.py --public-health-url https://...
    python3 scripts/update_solver_production.py --no-build
    python3 scripts/update_solver_production.py --skip-smoke
    python3 scripts/update_solver_production.py --skip-github

On helper failure the script restores the previous shared solver pair on the
server and reverts the GitHub variable to its prior value, leaving the system
in the exact pre-deployment state.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import subprocess
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

SSH_TARGET = "root@114.66.55.78"
REPOSITORY = "KnightCodeSquareMatrix/RIIC-Web"
ENV_VAR_NAME = "DEPLOY_APPROVED_SOLVER_SHA256"
LOCAL_ELF = ROOT / "target" / "release" / "infra-cli"

ENVIRONMENTS = {
    "production": {
        "gh_environment": "production",
        "app_root": "/opt/arknights-infra",
        "service": "arknights-infra",
        "port": 4175,
        "debug_tools": "0",
        "rate_limit": "1",
    },
    "development": {
        "gh_environment": "development",
        "app_root": "/opt/arknights-infra-dev",
        "service": "arknights-infra-dev",
        "port": 4275,
        "debug_tools": "0",
        "rate_limit": "1",
    },
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(cmd: list[str], *, stdin: str | None = None, check: bool = True) -> subprocess.CompletedProcess:
    print(f"\n$ {shlex.join(cmd)}")
    return subprocess.run(cmd, input=stdin, text=True, check=check)


def ensure_elf(path: Path) -> None:
    with path.open("rb") as f:
        magic = f.read(4)
    if magic != b"\x7fELF":
        raise SystemExit(f"{path} is not a Linux ELF. Do not upload infra-cli.exe.")


def gh_environment_variable(gh_environment: str, value: str | None) -> str:
    url = (
        f"repos/{REPOSITORY}/environments/{gh_environment}/variables/{ENV_VAR_NAME}"
    )
    if value is None:
        result = subprocess.run(
            ["gh", "api", url, "--jq", ".value"], text=True, capture_output=True, check=True
        )
        return result.stdout.strip()
    subprocess.run(["gh", "api", "--method", "PATCH", url, "-f", f"value={value}"], check=True)
    return value


def smoke_test(local_elf: Path, expected_sha256: str) -> None:
    request = '{"id":"deploy-smoke","method":"ping","params":{}}\n'
    probe = subprocess.run(
        [str(local_elf), "serve"],
        input=request,
        text=True,
        capture_output=True,
        timeout=60,
        check=True,
    )
    for line in probe.stdout.splitlines():
        if not line.strip():
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        result = payload.get("result")
        if not isinstance(result, dict):
            continue
        schemas = result.get("supported_plan_schema_versions")
        checks = {
            "ok": payload.get("ok") is True,
            "protocol_version": result.get("protocol_version") == 1,
            "schema_support": isinstance(schemas, list) and 3 in schemas,
            "fingerprint": result.get("solver_executable_sha256") == expected_sha256,
        }
        print(f"Smoke ping: {json.dumps({k: v for k, v in checks.items()}, indent=2)}")
        print(
            f"  git_commit={result.get('solver_git_commit')} "
            f"built_at={result.get('solver_built_at')}"
        )
        if not all(checks.values()):
            raise SystemExit("Local smoke test failed; refusing to deploy.")
        return
    raise SystemExit("Local smoke test produced no protocol response.")


def remote_deploy_script(
    *,
    environment: str,
    local_elf_sha256: str,
    public_health_url: str,
) -> str:
    cfg = ENVIRONMENTS[environment]
    app_root = cfg["app_root"]
    service = cfg["service"]
    port = cfg["port"]
    debug_tools = cfg["debug_tools"]
    rate_limit = cfg["rate_limit"]
    remote_tmp = "/root/infra-cli.new"
    trusted_bin = f"{app_root}/shared/bin"
    trusted_solver = f"{trusted_bin}/infra-cli"
    sidecar = f"{trusted_bin}/infra-cli.sha256"
    helper = "/usr/local/sbin/arknights-infra-deploy"
    repository = REPOSITORY
    public_health_arg = public_health_url or ""

    return textwrap.dedent(
        f"""
        set -euo pipefail

        app_root={shlex.quote(app_root)}
        trusted_bin={shlex.quote(trusted_bin)}
        trusted_solver={shlex.quote(trusted_solver)}
        sidecar={shlex.quote(sidecar)}
        remote_tmp={shlex.quote(remote_tmp)}
        expected_sha256={shlex.quote(local_elf_sha256)}
        public_health_arg={shlex.quote(public_health_arg)}

        echo "=== 0. upload sanity ==="
        test -f "$remote_tmp"
        test "$(sha256sum "$remote_tmp" | cut -d ' ' -f 1)" = "$expected_sha256"
        test "$(od -An -tx1 -N4 "$remote_tmp" | tr -d ' \\n')" = "7f454c46"

        echo "=== 1. read active release identity ==="
        current_release=$(readlink -f "$app_root/current")
        case "$current_release" in
          "$app_root"/releases/*) ;;
          *) echo "current 指向了 release 根目录之外：$current_release" >&2; exit 1 ;;
        esac
        manifest="$current_release/.release-artifact.json"
        test -f "$manifest"
        release_sha=$(node -e 'const m=JSON.parse(require("node:fs").readFileSync(process.argv[1],"utf8"));process.stdout.write(m.releaseSha)' "$manifest")
        release_tree_sha=$(node -e 'const m=JSON.parse(require("node:fs").readFileSync(process.argv[1],"utf8"));process.stdout.write(m.releaseTreeSha)' "$manifest")
        [[ "$release_sha" =~ ^[0-9a-f]{{40}}$ ]]
        [[ "$release_tree_sha" =~ ^[0-9a-f]{{40}}$ ]]
        echo "release_sha=$release_sha"
        echo "release_tree_sha=$release_tree_sha"
        archive=/tmp/arknights-infra-{environment}-"$release_sha".tar.gz

        echo "=== 2. back up current shared solver pair ==="
        old_sha=$(sha256sum "$trusted_solver" | cut -d ' ' -f 1)
        ts=$(date -u +%Y%m%dT%H%M%SZ)
        backup_dir="$app_root/shared/solver-backups/$ts-$old_sha"
        install -d -m 0700 -o root -g root "$backup_dir"
        cp -a "$trusted_solver" "$backup_dir/infra-cli"
        cp -a "$sidecar" "$backup_dir/infra-cli.sha256"
        echo "backup_dir=$backup_dir"

        restore_complete=0
        restore_shared() {{
          echo "Restoring previous shared solver pair from $backup_dir." >&2
          install -m 0755 -o root -g root "$backup_dir/infra-cli" "$trusted_bin/infra-cli.new"
          printf '%s\\n' "$old_sha" > "$sidecar.new"
          chmod 0644 "$sidecar.new"
          mv -Tf "$trusted_bin/infra-cli.new" "$trusted_solver"
          mv -Tf "$sidecar.new" "$sidecar"
          test "$(sha256sum "$trusted_solver" | cut -d ' ' -f 1)" = "$old_sha"
          test "$(cat "$sidecar")" = "$old_sha"
          echo "restored_shared_sha=$old_sha" >&2
        }}
        trap 'if [[ "$restore_complete" == "0" && -f "$backup_dir/infra-cli" ]]; then restore_shared; fi' EXIT

        echo "=== 3. atomic shared solver replacement ==="
        install -m 0755 -o root -g root "$remote_tmp" "$trusted_bin/infra-cli.new"
        printf '%s\\n' "$expected_sha256" > "$sidecar.new"
        chmod 0644 "$sidecar.new"
        mv -Tf "$trusted_bin/infra-cli.new" "$trusted_solver"
        mv -Tf "$sidecar.new" "$sidecar"
        test "$(sha256sum "$trusted_solver" | cut -d ' ' -f 1)" = "$expected_sha256"
        test "$(cat "$sidecar")" = "$expected_sha256"
        stat -c "solver %a %U:%G" "$trusted_solver"
        stat -c "sidecar %a %U:%G" "$sidecar"

        echo "=== 4. derive release archive from active release ==="
        rm -f "$archive"
        tar -C "$current_release" --sort=name --owner=0 --group=0 --numeric-owner --format=gnu \
          --exclude="./.release-sha" \
          --exclude="./.next/cache" \
          --exclude="./.next/standalone/.next/cache" \
          --exclude="./.next/standalone/.env.local" \
          --exclude="./.next/standalone/.env.production.local" \
          --exclude="./.next/standalone/bin" \
          -cf - . | gzip --best --no-name > "$archive"
        gzip -t "$archive"
        if tar -tzf "$archive" | grep -Eq "./\\.release-sha$|standalone/bin/|standalone/\\.env"; then
          echo "Release archive contains helper-owned paths." >&2
          exit 1
        fi

        echo "=== 5. activate through the deployment helper ==="
        if ! {helper} {environment} "$release_sha" "$archive" \\
            "$app_root" {service} arkinfra {port} "$public_health_arg" \\
            {debug_tools} {rate_limit} {repository} "$expected_sha256" "$release_tree_sha"; then
          echo "Deployment helper failed; the shared solver pair will be restored on exit." >&2
          exit 1
        fi
        restore_complete=1

        echo "=== 6. final consistency check ==="
        current_release=$(readlink -f "$app_root/current")
        installed_solver="$current_release/.next/standalone/bin/infra-cli"
        env_file="$current_release/.next/standalone/.env.production.local"
        test -f "$installed_solver"
        test -f "$env_file"
        test "$(sha256sum "$trusted_solver" | cut -d ' ' -f 1)" = "$expected_sha256"
        test "$(cat "$sidecar")" = "$expected_sha256"
        test "$(sha256sum "$installed_solver" | cut -d ' ' -f 1)" = "$expected_sha256"
        grep -Fx "INFRA_CLI_EXPECTED_SHA256=$expected_sha256" "$env_file" >/dev/null
        echo "shared=$(sha256sum "$trusted_solver" | cut -d ' ' -f 1)"
        echo "sidecar=$(cat "$sidecar")"
        echo "release_solver=$(sha256sum "$installed_solver" | cut -d ' ' -f 1)"
        echo "env_fingerprint=$(grep -F 'INFRA_CLI_EXPECTED_SHA256=' "$env_file" | cut -d= -f2)"
        systemctl is-active --quiet {service}
        health=$(curl -fsS --max-time 5 http://127.0.0.1:{port}/api/health)
        echo "$health" | node -e '
          let input = "";
          process.stdin.on("data", (chunk) => input += chunk);
          process.stdin.on("end", () => {{
            const body = JSON.parse(input);
            const queue = body?.data?.taskQueue;
            if (body?.success !== true
              || body?.data?.plannerReady !== true
              || queue?.ready !== true
              || queue?.releaseMatched !== true) process.exit(1);
          }});
        '
        echo "health=ready"
        echo "deployed_release=$current_release"
        """
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--environment",
        choices=sorted(ENVIRONMENTS),
        default="production",
        help="target environment (default: production)",
    )
    parser.add_argument("--ssh-target", default=SSH_TARGET, help="ssh target host")
    parser.add_argument(
        "--local-elf",
        type=Path,
        default=LOCAL_ELF,
        help="local Linux infra-cli ELF to deploy",
    )
    parser.add_argument(
        "--public-health-url",
        default="",
        help="optional public HTTPS health URL checked after activation",
    )
    parser.add_argument("--no-build", action="store_true", help="skip the cargo build step")
    parser.add_argument("--skip-smoke", action="store_true", help="skip the local serve smoke test")
    parser.add_argument(
        "--skip-github",
        action="store_true",
        help="do not update the GitHub Environment approval variable",
    )
    return parser.parse_args(argv)


def build(local_elf: Path) -> None:
    dirty = subprocess.run(
        ["git", "diff", "--quiet", "--", "crates/", "Cargo.toml", "Cargo.lock", "build.rs"],
        cwd=ROOT,
    ).returncode
    if dirty != 0:
        raise SystemExit(
            "crates/ has uncommitted changes; commit or stash them first so the "
            "artifact is uniquely identified by its Git commit."
        )
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, capture_output=True, check=True
    ).stdout.strip()
    built_at = subprocess.run(
        ["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"], text=True, capture_output=True, check=True
    ).stdout.strip()
    print(f"[BUILD] git_commit={commit} built_at={built_at}")
    run(
        ["cargo", "build", "--locked", "--release", "-p", "infra-cli"],
        env={**os.environ, "SOLVER_GIT_COMMIT": commit, "SOLVER_BUILT_AT": built_at},
    )
    if not local_elf.is_file():
        raise SystemExit(f"build did not produce {local_elf}")


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    cfg = ENVIRONMENTS[args.environment]

    local_elf = args.local_elf.expanduser().resolve()
    if not args.no_build:
        build(local_elf)
    elif not local_elf.is_file():
        raise SystemExit(f"local ELF does not exist: {local_elf}")
    ensure_elf(local_elf)

    expected_sha256 = sha256(local_elf)
    print(f"Environment: {args.environment}")
    print(f"Local ELF: {local_elf}")
    print(f"Local sha256: {expected_sha256}")

    if not args.skip_smoke:
        smoke_test(local_elf, expected_sha256)

    previous_sha256 = ""
    if not args.skip_github:
        previous_sha256 = gh_environment_variable(cfg["gh_environment"], None)
        print(f"GitHub {cfg['gh_environment']} {ENV_VAR_NAME}: {previous_sha256} -> {expected_sha256}")
        gh_environment_variable(cfg["gh_environment"], expected_sha256)

    try:
        run(["scp", str(local_elf), f"{args.ssh_target}:/root/infra-cli.new"])
        script = remote_deploy_script(
            environment=args.environment,
            local_elf_sha256=expected_sha256,
            public_health_url=args.public_health_url,
        )
        run(["ssh", args.ssh_target, "bash", "-s"], stdin=script)
    except subprocess.CalledProcessError:
        if previous_sha256 and not args.skip_github:
            print(f"Deployment failed; reverting GitHub {ENV_VAR_NAME} to {previous_sha256}")
            gh_environment_variable(cfg["gh_environment"], previous_sha256)
        raise

    if args.public_health_url:
        import urllib.request

        with urllib.request.urlopen(args.public_health_url, timeout=30) as response:
            payload = json.load(response)
        if payload.get("success") is not True or payload.get("data", {}).get("plannerReady") is not True:
            raise SystemExit(f"public health check failed: {args.public_health_url}")

    print(f"[RESULT] {args.environment} solver deployed: {expected_sha256}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode) from exc
