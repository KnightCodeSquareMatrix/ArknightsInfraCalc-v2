---
name: arknights-solver-deploy
description: 把本地最新 Linux infra-cli 投递到远端 quality-check（114.66.55.78），只确保 ELF 原子进入远端 runner 输入目录。用户要求"推送求解器""更新求解器"或"deploy solver"时使用。
---

# Arknights 远端 Quality Check 求解器投递

求解器投递和批量验收默认目标是远端 quality-check：`root@114.66.55.78`。
唯一入口为无参数的 `scripts/update_solver_quality.py`；它以原子
rename 投递 Linux ELF，由远端 runner 异步安装不可变 bundle；该入口不创建 job。

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

1. 确认当前 worktree 是要发布的完整源码；需要 clean commit 语义时使用 clean / 隔离 worktree。
2. 运行唯一入口：`python3 scripts/update_solver_quality.py`。
   - 脚本自行注入 commit / builtAt 并执行一次 release 构建；不要在调用前重复构建。
   - 脚本使用带 ELF SHA 的临时文件，在远端锁保护下检查目标不存在并完成原子 handoff；runner 可能立即取走 final 文件。
   - 目标已存在或锁等待超时会 fail-closed，不覆盖、不删除已有制品。
   - 脚本不等待 bundle，也不创建或等待 quality job。
   - 不手工覆盖已有 bundle、运行中的 CLI 或共享 data 目录。
3. 异步验证：公开 status API 确认新 bundle 的 `gitCommit` 与 `executableSha256`；
   bundle definition 的 `fingerprint.dataMode=embedded` 才是热路径证据。公开 status
   摘要不提供 `embeddedData`，不得把字段缺失判成失败。详情按
   `docs/QUALITY_CHECK_CLI_UPDATE.md` §6 执行；批量 job 需另行显式创建。

## 失败处理

- 投递文件长期停留在 incoming、discovery processing 报错或显式 job 失败时，按
  `docs/QUALITY_CHECK_CLI_UPDATE.md` §7 处理，不重传或覆盖已有 bundle。
- runner 不可用时先修复远端质量服务，再投递；不要切回本地 Check 作为静默 fallback。

## 边界

- 该流程只投递质量验证用 CLI，不更新主站或 dev 应用的 `bin/infra-cli`。
- quality-check 默认走 `embeddedData=true` 热路径；不要自行运行 bake 或上传外部 data。
- 机制/数据语义问题转 `arknights-maintenance`；新功能转 `arknights-feature`。
