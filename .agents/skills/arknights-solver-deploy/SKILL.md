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
release 由部署 helper 在激活时从 shared 制品注入求解器与指纹，
release 归档本身不得携带求解器。

## 环境事实（真源：systemd 运行时与部署 helper）

- 服务器：`root@114.66.55.78`。
- 部署 helper：`/usr/local/sbin/arknights-infra-deploy`（root:root 0755，契约 v7）。
- 网站仓库：`Closure-RIIC/RIIC-Web`；部署源标识与 GitHub Environment 均以此仓库为准。
- release manifest 的 `kind` 决定运行时目录：`riic-web-tanstack-start` 使用
  `.output`，`riic-web-standalone` 使用 `.next/standalone`。归档须排除对应目录
  下的 `bin`、`.env*` 与运行时 cache；TanStack 还须排除旧 `.next` 根目录。
- 共享求解器：`<app_root>/shared/bin/infra-cli`（root:root 0755），
  同行 `infra-cli.sha256`（root:root 0644，单行小写 SHA-256）。
- 外置运行时数据：`<app_root>/shared/bin-data`，由 helper 注入 release 的
  `bin/data`。网站优先使用这些数据，不能只更新 ELF 而保留旧技能与规则。
  `shared/bin/data` 链接到 `../bin-data`，使 active-cli 指向 shared ELF 时也
  使用同版外置数据，避免 root 探活生成的内置缓存阻止服务账户启动。
  部署脚本从当前提交打包 `data/`，排除 `baked/`、反馈种子和收集的 Box；
  `fixtures/` 仅保留引擎运行时依赖的标准 `243/`，
  携带逐文件 `.solver-data.sha256` 和来源 `.solver-data-commit`，激活后核对
  shared 与 release 两份数据。
- GitHub Environment 变量：`DEPLOY_APPROVED_SOLVER_SHA256`（production /
  development 各自独立），批准摘要必须与 shared 制品、sidecar、release 内
  求解器、release `.env.production.local` 的 `INFRA_CLI_EXPECTED_SHA256`
  及 Worker `ping` 指纹五方一致。

## 部署步骤

1. 确认当前 worktree 的求解器源码与 `data/` 无未提交改动；脚本会拒绝不一致构建。
   默认制品路径为 `$CARGO_TARGET_DIR/release/infra-cli`，未设置时使用仓库
   `target/release/infra-cli`。
2. 运行唯一入口：

   ```bash
   python3 scripts/update_solver_production.py
   ```

   脚本自动依次执行：

   - 注入 `SOLVER_GIT_COMMIT` / `SOLVER_BUILT_AT` 执行 `cargo build --release`；
   - 本地 `serve` 冒烟（protocol 1、schema 支持、ELF 指纹）；
   - 从当前提交打包运行时数据，更新 GitHub Environment 批准摘要；
   - scp 上传并在远端替换 shared 制品、sidecar
     与 `bin-data`（旧品自动备份到
     `<app_root>/shared/solver-backups/<ts>-<old-sha>/`）；
   - 从当前 release 派生归档并调用部署 helper 激活（网站与 plan worker
     重启、内部健康与生产 5 分钟观测、失败自动回滚）。

   当前 Core 与 CLI 的运行时 Bake 均已关闭，普通 `serve` 不加载、预热或查询
   Bake；本地部署冒烟显式使用 `serve --no-bake`。部署不生成或投递 Bake 表。

3. 生产公网健康入口需要显式给出：

   ```bash
   python3 scripts/update_solver_production.py --public-health-url https://.../api/health
   ```

   可选覆盖：`--environment development`、`--local-elf <path>`、
   `--ssh-target root@<host>`、`--no-build`、`--skip-smoke`、`--skip-github`。

## 失败处理

- helper 失败时它会自动回滚到上一 release；脚本随后把 shared 制品、运行时数据与
  GitHub 变量恢复为部署前值，系统回到完全一致状态。
- 远端前台部署完成回滚并以专用状态码 76 结束后，再核对 shared、sidecar、
  current release 与环境指纹，确认旧制品后才恢复旧 GitHub 审批；瞬时读到旧摘要
  不能证明仍在运行的部署已结束。helper 已激活后若只读复核失败，保留新版本及
  其批准摘要并报错，不能把已激活的新制品配上旧审批；连接中断且状态不明时也不
  盲目撤回审批，须先恢复连接并核对现场。
- 需要手动回滚时，从 `shared/solver-backups/<ts>-<old-sha>/` 恢复
  `infra-cli` 与 `infra-cli.sha256`（校验 root:root、ELF magic），再把
  备份的 `bin-data` 恢复为 `shared/bin-data`，
  GitHub 变量改回旧摘要，最后重跑一次部署 helper 让新 release 携带旧制品。
- 不要重传或覆盖已有 backup 目录；不要手工改写运行中 release 内的文件。

## 边界

- 该流程只更新主站与开发环境的共享求解器，不触碰 quality-check 的
  独立 bundle 投递；quality-check 更新见 `scripts/update_solver_quality.py`
  与 `docs/质量与维护/QUALITY_CHECK_CLI_UPDATE.md`。
- 网站 release 发布由前端仓库 GitHub Actions 驱动；本仓库只负责求解器制品
  与批准摘要，不构建或推送网站代码。
- 机制/数据语义问题和新功能不在部署流程内，按根 `AGENTS.md` 另起任务。
