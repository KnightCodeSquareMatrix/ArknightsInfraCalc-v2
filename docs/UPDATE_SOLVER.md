# 更新服务器求解器（dev 环境）

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/FRONTEND_SERVE_GUIDE.md
> 摘要：把本地 Linux 版 infra-cli 推送到 dev 服务器 114.66.55.78 的原子替换、重启、健康检查与自动回滚流程

> 说明：生产环境（`/opt/arknights-infra`，端口 4175）的更新流程仍以
> [前端仓库 UPDATE_SOLVER.md](https://github.com/KnightCodeSquareMatrix/ArknightsInfraCalc-v2_beta_test_frontend/blob/develop/docs/UPDATE_SOLVER.md) 为真源。
> 本页只负责 dev 环境（`/opt/arknights-infra-dev`）。不要混淆两台环境；推送前先确认目标 app 目录与端口。

## 1. 环境事实（真源：systemd 运行时）

| 项 | 值 |
|---|---|
| 服务器 | `root@114.66.55.78`，SSH 密钥 `~/.ssh/id_rsa`（默认提供） |
| 应用根 | `/opt/arknights-infra-dev` |
| 软链 | `/opt/arknights-infra-dev/current` |
| 求解器 | `/opt/arknights-infra-dev/current/bin/infra-cli` |
| systemd 服务 | `arknights-infra-dev.service` |
| 内部 Next 端口 | `4275`（仅 127.0.0.1） |
| 环境文件 | `/opt/arknights-infra-dev/current/.env.production.local`（`INFRA_CLI_EXPECTED_SHA256`） |
| 回滚备份 | `/opt/arknights-infra-dev/solver-backups/`（root-only） |

运行时数据：dev 的 `bin/data` 当前只含 `baked` 目录，无外部核心数据文件，
因此求解器使用内置数据，替换 CLI 不会与旧数据失配。若未来 dev 的
`bin/data` 出现任何非 `baked` 文件，则必须成套更新 Worker 数据文件或改走完整前端 release。

## 2. 自动化脚本（推荐）

```bash
# 先构建 Linux 版
cargo build --release -p infra-cli

# 推送到 dev：上传 + 原子替换 + 重启 + 健康检查 + 失败自动回滚
python3 scripts/update_solver_dev.py
```

脚本默认即 dev 环境；可选参数覆盖见脚本 `--help`。脚本全程 `set -euo pipefail`，
替换后先用 `Usage:` 探测新二进制，再重启服务，最后轮询健康接口
`http://127.0.0.1:4275/api/health` 直到 `success=true` 且 `plannerReady=true`。
任何一步失败都会成套恢复旧 CLI 与旧指纹并再次重启，不留下半套状态。

## 3. 手工流程（脚本失效时对照执行）

在本地或构建机准备 Linux 版 `infra-cli`。不要上传 Windows 版 `infra-cli.exe`。

```bash
sha256sum ./infra-cli
scp ./infra-cli root@114.66.55.78:/root/infra-cli.new
```

登录服务器后执行原子替换与重启，命令与 `scripts/update_solver_dev.py` 生成的远端脚本一致，
要点：

1. 校验 `current` 软链指向 `releases/*`，`bin/infra-cli`、`.env.production.local` 存在且非软链。
2. 以 `runuser -u arkinfra` 读取旧 CLI 与 env 到私有备份目录，校验 owner 为 `0:0`、ELF magic。
3. 计算新 SHA 写入 `.env.production.local` 的 `INFRA_CLI_EXPECTED_SHA256`。
4. 原子 `mv` 换入 staged 文件；`runuser` 探测 `Usage:`。
5. `systemctl restart arknights-infra-dev`；失败即调用 `restore_previous`。
6. 30 秒内轮询 `curl -fsS http://127.0.0.1:4275/api/health`，校验 `success=true` 且 `data.plannerReady=true`。
7. 健康失败同样成套回滚并再次重启。

## 4. 发布后验证

```bash
ssh root@114.66.55.78 'systemctl is-active arknights-infra-dev'
ssh root@114.66.55.78 'ss -ltnp | grep 4275'
ssh root@114.66.55.78 'curl -fsS http://127.0.0.1:4275/api/health'
ssh root@114.66.55.78 'grep INFRA_CLI_EXPECTED_SHA256 /opt/arknights-infra-dev/current/.env.production.local'
ssh root@114.66.55.78 'journalctl -u arknights-infra-dev -n 80 --no-pager'
```

健康结果至少满足：

- `success: true`
- `data.status` 为 `ready`
- `data.plannerReady` 为 `true`

## 5. 回滚

脚本失败时自动回滚，备份目录为
`/opt/arknights-infra-dev/solver-backups/update-<timestamp>.<random>`，
含 `infra-cli` 与 `.env.production.local` 两份原始文件。手动回滚到指定备份：

```bash
set -euo pipefail

app_root=/opt/arknights-infra-dev
app=$(readlink -f -- "$app_root/current")
backup_dir="$app_root/solver-backups/update-<timestamp>.<random>"
backup="$backup_dir/infra-cli"
env_backup="$backup_dir/.env.production.local"

case "$app" in "$app_root"/releases/*) ;; *) exit 1 ;; esac
test -f "$backup"
test -f "$env_backup"
test ! -L "$backup"
test ! -L "$env_backup"
test "$(stat -c '%u:%g' "$backup")" = "0:0"
test "$(od -An -tx1 -N4 "$backup" | tr -d ' \n')" = "7f454c46"
install -o arkinfra -g arkinfra -m 0755 "$backup" "$backup_dir/infra-cli.restore"
install -o arkinfra -g arkinfra -m 0600 "$env_backup" "$backup_dir/env.restore"
mv -Tf "$backup_dir/infra-cli.restore" "$app/bin/infra-cli"
mv -Tf "$backup_dir/env.restore" "$app/.env.production.local"
systemctl restart arknights-infra-dev
curl -fsS http://127.0.0.1:4275/api/health | node -e '
  let input = "";
  process.stdin.on("data", chunk => input += chunk);
  process.stdin.on("end", () => {
    const body = JSON.parse(input);
    if (body?.success !== true || body?.data?.plannerReady !== true) process.exit(1);
  });
'
```

## 6. 避免被下次 dev 发布覆盖

服务器上的替换只影响当前 release。下次从仓库发布 dev 前端时，部署包里的
`bin/infra-cli` 可能覆盖它。确认新求解器稳定后，还应把同一 Linux 二进制
合并进前端仓库 dev 分支的 `bin/infra-cli`，通过 PR 发布后再部署一次。
