---
name: arknights-solver-deploy
description: 把本地 Linux infra-cli 部署到线上主站或开发服务器（114.66.55.78）的共享求解器架构：shared/bin 原子替换 + SHA-256 sidecar + GitHub Environment 批准摘要 + 部署 helper 激活与自动回滚。用户要求"推送求解器""更新求解器""部署求解器到网站/服务器""deploy solver"时使用。
---

# Arknights 求解器线上部署（shared 制品架构）

求解器部署唯一入口为 `scripts/update_solver_production.py`。它把当前 worktree
的 Linux ELF 部署到目标环境的共享求解器架构：

- 默认目标为生产主站 `/opt/arknights-infra`（riic.autos），服务
  `arknights-infra` + `arknights-infra-worker`，内部端口 `4175`。
- `--environment development` 目标为 `/opt/arknights-infra-dev`，服务
  `arknights-infra-dev` + `arknights-infra-dev-worker`，内部端口 `4275`。

不要按旧文档手工替换 release 内的 `bin/infra-cli` 或 `.env.production.local`；
standalone release 由部署 helper 在激活时从 shared 制品注入求解器与指纹，
release 归档本身不得携带求解器。

## 环境事实（真源：systemd 运行时与部署 helper）

- 服务器：`root@114.66.55.78`。
- 部署 helper：`/usr/local/sbin/arknights-infra-deploy`（root:root 0755，契约 v6）。
- 共享求解器：`<app_root>/shared/bin/infra-cli`（root:root 0755），
  同行 `infra-cli.sha256`（root:root 0644，单行小写 SHA-256）。
- GitHub Environment 变量：`DEPLOY_APPROVED_SOLVER_SHA256`（production /
  development 各自独立），批准摘要必须与 shared 制品、sidecar、release 内
  求解器、release `.env.production.local` 的 `INFRA_CLI_EXPECTED_SHA256`
  及 Worker `ping` 指纹五方一致。

## 部署步骤

1. 确认当前 worktree 的 `crates/` 无未提交改动；脚本会拒绝不一致构建。
2. 运行唯一入口：

   ```bash
   python3 scripts/update_solver_production.py
   ```

   脚本自动依次执行：

   - 注入 `SOLVER_GIT_COMMIT` / `SOLVER_BUILT_AT` 执行 `cargo build --release`；
   - 本地 `serve` 冒烟（protocol 1、schema 支持、ELF 指纹）；
   - 更新 GitHub Environment 批准摘要；
   - scp 上传并在远端原子替换 shared 制品与 sidecar（旧品自动备份到
     `<app_root>/shared/solver-backups/<ts>-<old-sha>/`）；
   - 从当前 release 派生归档并调用部署 helper 激活（网站与 plan worker
     重启、内部健康与生产 5 分钟观测、失败自动回滚）。

3. 生产公网健康入口需要显式给出：

   ```bash
   python3 scripts/update_solver_production.py --public-health-url https://.../api/health
   ```

   可选覆盖：`--environment development`、`--local-elf <path>`、
   `--ssh-target root@<host>`、`--no-build`、`--skip-smoke`、`--skip-github`。

## 失败处理

- helper 失败时它会自动回滚到上一 release；脚本随后把 shared 制品与
  GitHub 变量恢复为部署前值，系统回到完全一致状态。
- 需要手动回滚时，从 `shared/solver-backups/<ts>-<old-sha>/` 恢复
  `infra-cli` 与 `infra-cli.sha256`（校验 root:root、ELF magic），再把
  GitHub 变量改回旧摘要，最后重跑一次部署 helper 让新 release 携带旧制品。
- 不要重传或覆盖已有 backup 目录；不要手工改写运行中 release 内的文件。

## 边界

- 该流程只更新主站与开发环境的共享求解器，不触碰 quality-check 的
  独立 bundle 投递；quality-check 更新见 `scripts/update_solver_quality.py`
  与 `docs/QUALITY_CHECK_CLI_UPDATE.md`。
- 网站 release 发布由前端仓库 GitHub Actions 驱动；本仓库只负责求解器制品
  与批准摘要，不构建或推送网站代码。
- 机制/数据语义问题和新功能不在部署流程内，按根 `AGENTS.md` 另起任务。
