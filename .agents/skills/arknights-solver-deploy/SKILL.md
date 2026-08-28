---
name: arknights-solver-deploy
description: 把本地最新 Linux infra-cli 投递到远端 quality-check（114.66.55.78），由 Check 创建不可变 bundle 并执行批量验收。用户要求"推送求解器""更新求解器""批量验收新求解器"或"deploy solver"时使用。
---

# Arknights 远端 Quality Check 求解器投递

求解器投递和批量验收默认目标是远端 quality-check：`root@114.66.55.78`。
唯一入口为无参数的 `scripts/update_solver_quality.py`；它以原子
rename 投递 Linux ELF，由远端 runner 安装不可变 bundle 并创建质量矩阵 job。

不要默认投递到本地 Check，也不要替换 `/opt/arknights-infra-dev` 中的应用 CLI。
只有用户明确要求 dev 应用部署时，才改用 `scripts/update_solver_dev.py`；它不等同于
quality-check 的 bundle 投递或批量验收。

## 远端质量服务事实（真源：systemd 运行时）

- 服务器：`root@114.66.55.78`，SSH 密钥 `~/.ssh/id_rsa`（默认提供）。
- Check 应用根：`/opt/arknights-infra-quality-next/current`。
- 投递目录：`/var/lib/arknights-infra-quality/solver-incoming/`。
- 服务：`arknights-infra-quality-next.service` 与
  `arknights-infra-quality-next-runner.service`，Next 仅监听 `127.0.0.1:4385`。
- 远端 API：`https://instance-pi2ohhfj.tail2dca9.ts.net:10000`。
- 完整协议、存储路径和故障处理以 `docs/QUALITY_CHECK_CLI_UPDATE.md` 为准。

## 推送步骤

1. 构建：`cargo build --release -p infra-cli`（Linux；不要上传 `infra-cli.exe`）。
2. 投递：`python3 scripts/update_solver_quality.py`。
   - 脚本固定投递到远端并等待 bundle 安装和 quality job 终态。
   - 不手工覆盖已有 bundle、运行中的 CLI 或共享 data 目录。
3. 验证：确认新 bundle 的 `gitCommit`、`executableSha256` 与 `embeddedData: true`，
   且其质量 job 完成，没有新增失败 case。详情按
   `docs/QUALITY_CHECK_CLI_UPDATE.md` §6 执行。

## 失败处理

- 投递文件长期停留在 incoming、进入 failed 或 job 未出现时，按
  `docs/QUALITY_CHECK_CLI_UPDATE.md` §7 处理，不重传或覆盖已有 bundle。
- runner 不可用时先修复远端质量服务，再投递；不要切回本地 Check 作为静默 fallback。

## 边界

- 该流程只投递质量验证用 CLI，不更新主站或 dev 应用的 `bin/infra-cli`。
- quality-check 默认走 `embeddedData=true` 热路径；不要自行运行 bake 或上传外部 data。
- 机制/数据语义问题转 `arknights-maintenance`；新功能转 `arknights-feature`。
