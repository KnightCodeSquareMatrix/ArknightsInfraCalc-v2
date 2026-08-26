---
name: arknights-solver-deploy
description: 把本地最新 Linux infra-cli 推送到 dev 服务器（114.66.55.78 /opt/arknights-infra-dev，端口 4275），含原子替换、重启、健康检查和失败自动回滚。用户要求"推送到 dev""推送求解器到 dev""更新 dev 求解器""deploy dev solver""上线 dev solver"时使用。
---

# Arknights dev 求解器推送

dev 环境推送唯一入口：`scripts/update_solver_dev.py`。生产环境（`/opt/arknights-infra`，
端口 4175）走前端仓库 `docs/UPDATE_SOLVER.md`，不要混用。

## 服务器事实（真源：systemd 运行时）

- 服务器：`root@114.66.55.78`，SSH 密钥 `~/.ssh/id_rsa`（默认提供）。
- 应用根：`/opt/arknights-infra-dev`，`current` 软链指向 `releases/<ts>-<hash>`。
- 求解器：`/opt/arknights-infra-dev/current/bin/infra-cli`。
- 服务：`arknights-infra-dev.service`，内部 Next 端口 `4275`（仅 127.0.0.1）。
- 环境文件：`.env.production.local`，其中 `INFRA_CLI_EXPECTED_SHA256` 与二进制指纹强绑定。
- 回滚备份：`/opt/arknights-infra-dev/solver-backups/`（root-only）。

## 推送步骤

1. 构建：`cargo build --release -p infra-cli`（Linux；不要上传 `infra-cli.exe`）。
2. 推送：`python3 scripts/update_solver_dev.py`。
   - 脚本先 scp 到 `/root/infra-cli.new`，再远端执行原子替换 + 重启 + 健康轮询，
     任一步失败自动成套回滚旧 CLI 与旧指纹。
   - 常用参数：`--no-restart`（只安装不重启）、`--local-elf <path>`。
3. 验证（脚本已自动做健康检查；手工复查）：
   - `ssh root@114.66.55.78 'systemctl is-active arknights-infra-dev'`
   - `ssh root@114.66.55.78 'ss -ltnp | grep 4275'`
   - `ssh root@114.66.55.78 'curl -fsS http://127.0.0.1:4275/api/health'`
   - 健康结果需 `success: true`、`data.status=ready`、`data.plannerReady=true`。
   - `ssh root@114.66.55.78 'grep INFRA_CLI_EXPECTED_SHA256 /opt/arknights-infra-dev/current/.env.production.local'`

## 失败与回滚

- 脚本失败会自动回滚，输出备份目录 `update-<timestamp>.<random>`。
- 手动回滚到指定备份的完整命令见 `docs/UPDATE_SOLVER.md` §5。
- 回滚后重新健康检查；若服务仍无法启动，保留现场并检查
  `journalctl -u arknights-infra-dev -n 80 --no-pager`。

## 边界

- 该流程只替换 dev 的 CLI，不更新 `bin/data`。dev 的 `bin/data` 当前只含 `baked`，
  使用内置数据；若未来出现非 `baked` 外部数据文件，必须成套更新或改走完整 release。
- 每次替换只影响当前 release；下次前端 dev 发布可能覆盖二进制。确认稳定后，
  应把同一 Linux 二进制合并进前端仓库 dev 分支的 `bin/infra-cli` 再发布。
- 机制/数据语义问题转 `arknights-maintenance`；新功能转 `arknights-feature`。
