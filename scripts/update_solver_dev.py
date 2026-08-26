#!/usr/bin/env python3
"""Push the local Linux infra-cli to the dev environment and verify it.

Implements the dev flavour of docs/UPDATE_SOLVER.md:

    cargo build --release -p infra-cli
    python3 scripts/update_solver_dev.py

Optional overrides:

    python3 scripts/update_solver_dev.py --target root@1.2.3.4
    python3 scripts/update_solver_dev.py --local-elf /some/path/infra-cli
    python3 scripts/update_solver_dev.py --app-root /opt/arknights-infra-dev
    python3 scripts/update_solver_dev.py --service arknights-infra-dev
    python3 scripts/update_solver_dev.py --port 4275
    python3 scripts/update_solver_dev.py --no-restart

Defaults match the dev environment on 114.66.55.78. Use --no-restart to only
install the solver and fingerprint without touching the service.
"""

from __future__ import annotations

import argparse
import hashlib
import shlex
import subprocess
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

DEFAULT_SSH_TARGET = "root@114.66.55.78"
DEFAULT_LOCAL_ELF = ROOT / "target" / "release" / "infra-cli"
DEFAULT_REMOTE_TMP = "/root/infra-cli.new"
DEFAULT_APP_ROOT = "/opt/arknights-infra-dev"
DEFAULT_SERVICE = "arknights-infra-dev"
DEFAULT_PORT = 4275
DEFAULT_APP_USER = "arkinfra"


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


def remote_update_script(
    app_root: str,
    new_cli: str,
    service: str,
    port: int,
    app_user: str,
    restart: bool,
) -> str:
    restart_block = ""
    if restart:
        restart_block = f"""
        if ! systemctl restart {shlex.quote(service)}; then
          restore_previous
          if systemctl restart {shlex.quote(service)}; then
            echo "服务重启失败，已恢复旧求解器和预期指纹：$backup_dir" >&2
          else
            echo "已恢复旧文件，但服务仍无法启动；请保留现场并检查 journal：$backup_dir" >&2
          fi
          exit 1
        fi

        healthy=false
        for _ in $(seq 1 30); do
          if curl -fsS http://127.0.0.1:{port}/api/health >"$health_file" \
            && node -e '
              const fs = require("node:fs");
              const body = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
              if (body?.success !== true || body?.data?.plannerReady !== true) process.exit(1);
            ' "$health_file"; then
            healthy=true
            break
          fi
          sleep 1
        done

        if [ "$healthy" != true ]; then
          restore_previous
          if systemctl restart {shlex.quote(service)}; then
            echo "健康检查失败，已恢复旧求解器和预期指纹：$backup_dir" >&2
          else
            echo "健康检查失败且恢复后服务仍无法启动；请保留现场并检查 journal：$backup_dir" >&2
          fi
          exit 1
        fi

        echo "Installed solver:"
        sha256sum "$installed_cli"
        cat "$health_file"
        echo "root-only 回滚备份：$backup_dir"
        """

    return (
        textwrap.dedent(
            f"""
            set -euo pipefail

            app_root={shlex.quote(app_root)}
            app_link="$app_root/current"
            app=$(readlink -f -- "$app_link")
            new_cli={shlex.quote(new_cli)}
            installed_cli="$app/bin/infra-cli"
            env_file="$app/.env.production.local"
            ts=$(date +%Y%m%d%H%M%S)
            backup_root="$app_root/solver-backups"
            install -d -o root -g root -m 0700 "$backup_root"
            backup_dir=$(mktemp -d "$backup_root/update-$ts.XXXXXX")
            chmod 0700 "$backup_dir"
            backup="$backup_dir/infra-cli"
            env_backup="$backup_dir/.env.production.local"
            staged="$backup_dir/infra-cli.staged"
            env_staged="$backup_dir/.env.production.local.staged"
            health_file="$backup_dir/health.json"

            test -L "$app_link"
            case "$app" in
              "$app_root"/releases/*) ;;
              *) echo "current 指向了 release 根目录之外：$app" >&2; exit 1 ;;
            esac
            test -f "$new_cli"
            test -f "$installed_cli"
            test -f "$env_file"
            test ! -L "$installed_cli"
            test ! -L "$env_file"

            runtime_data="$app/bin/data"
            runtime_data_files=(operator_instances.json skill_table.json base_systems.json training_advice_knowledge.json)
            runtime_data_present=0
            runtime_data_complete=1
            if [[ -L "$runtime_data" ]]; then
              runtime_data_present=1
              runtime_data_complete=0
            elif [[ -d "$runtime_data" ]] \\
              && [[ -n "$(find "$runtime_data" -mindepth 1 -maxdepth 1 ! -name baked -print -quit)" ]]; then
              runtime_data_present=1
            fi
            for required_file in "${{runtime_data_files[@]}}"; do
              if [[ ! -f "$runtime_data/$required_file" || -L "$runtime_data/$required_file" ]]; then
                runtime_data_complete=0
              fi
            done
            if [[ "$runtime_data_present" -eq 1 && "$runtime_data_complete" -ne 1 ]]; then
              echo "bin/data 是不完整的旧数据集；请改走完整前端 release，或先成套更新 Worker 数据文件。" >&2
              exit 1
            fi

            echo "Uploaded solver:"
            sha256sum "$new_cli"
            expected_solver_sha256=$(sha256sum "$new_cli" | cut -d ' ' -f 1)
            printf '%s\\n' "$expected_solver_sha256" | grep -Eq '^[0-9a-f]{{64}}$'

            runuser -u {shlex.quote(app_user)} -- cat -- "$installed_cli" > "$backup"
            chmod 0700 "$backup"
            test -f "$backup"
            test ! -L "$backup"
            test "$(stat -c '%u:%g' "$backup")" = "0:0"
            test "$(od -An -tx1 -N4 "$backup" | tr -d ' \\n')" = "7f454c46"
            echo "Current solver backup:"
            sha256sum "$backup"
            runuser -u {shlex.quote(app_user)} -- cat -- "$env_file" > "$env_backup"
            chmod 0600 "$env_backup"
            test -f "$env_backup"
            test ! -L "$env_backup"
            test "$(stat -c '%u:%g' "$env_backup")" = "0:0"
            install -o {shlex.quote(app_user)} -g {shlex.quote(app_user)} -m 0755 "$new_cli" "$staged"

            runuser -u {shlex.quote(app_user)} -- awk -v hash="$expected_solver_sha256" '
              BEGIN {{ written = 0 }}
              /^INFRA_CLI_EXPECTED_SHA256=/ {{
                print "INFRA_CLI_EXPECTED_SHA256=" hash
                written = 1
                next
              }}
              {{ print }}
              END {{
                if (!written) print "INFRA_CLI_EXPECTED_SHA256=" hash
              }}
            ' "$env_file" > "$env_staged"
            chown {shlex.quote(app_user)}:{shlex.quote(app_user)} "$env_staged"
            chmod 0600 "$env_staged"

            restore_previous() {{
              test -f "$backup" && test ! -L "$backup"
              test -f "$env_backup" && test ! -L "$env_backup"
              test "$(stat -c '%u:%g' "$backup")" = "0:0"
              test "$(stat -c '%u:%g' "$env_backup")" = "0:0"
              test "$(od -An -tx1 -N4 "$backup" | tr -d ' \\n')" = "7f454c46"
              install -o {shlex.quote(app_user)} -g {shlex.quote(app_user)} -m 0755 "$backup" "$backup_dir/infra-cli.restore"
              install -o {shlex.quote(app_user)} -g {shlex.quote(app_user)} -m 0600 "$env_backup" "$backup_dir/env.restore"
              mv -Tf "$backup_dir/infra-cli.restore" "$installed_cli"
              mv -Tf "$backup_dir/env.restore" "$env_file"
            }}

            mv -Tf "$staged" "$installed_cli"
            mv -Tf "$env_staged" "$env_file"

            cli_probe=$(runuser -u {shlex.quote(app_user)} -- "$installed_cli" 2>&1 || true)
            if ! printf '%s\\n' "$cli_probe" | grep -q '^Usage:'; then
              restore_previous
              echo "新求解器启动探测失败，已恢复：$backup_dir" >&2
              exit 1
            fi
            """
        ).strip()
        + "\n"
        + textwrap.dedent(restart_block).strip()
        + "\n"
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=DEFAULT_SSH_TARGET, help="ssh target host")
    parser.add_argument(
        "--local-elf",
        type=Path,
        default=DEFAULT_LOCAL_ELF,
        help="local Linux infra-cli ELF to upload",
    )
    parser.add_argument("--remote-tmp", default=DEFAULT_REMOTE_TMP, help="remote temp path")
    parser.add_argument("--app-root", default=DEFAULT_APP_ROOT, help="remote dev app root")
    parser.add_argument(
        "--service",
        default=DEFAULT_SERVICE,
        help="systemd service to restart (default: arknights-infra-dev)",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=DEFAULT_PORT,
        help="dev internal Next port for health check",
    )
    parser.add_argument(
        "--app-user",
        default=DEFAULT_APP_USER,
        help="remote service user that owns the release",
    )
    parser.add_argument(
        "--no-restart",
        action="store_true",
        help="install solver and fingerprint but do not restart the service",
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

    print(f"Local ELF: {local_elf}")
    print(f"Local sha256: {sha256(local_elf)}")
    print(f"SSH target: {args.target}")
    print(f"Remote app root: {args.app_root}")
    print(f"Service: {args.service}")

    run(["scp", str(local_elf), f"{args.target}:{args.remote_tmp}"])

    script = remote_update_script(
        app_root=args.app_root,
        new_cli=args.remote_tmp,
        service=args.service,
        port=args.port,
        app_user=args.app_user,
        restart=not args.no_restart,
    )
    run(["ssh", args.target, "bash", "-s"], stdin=script)

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode) from exc
