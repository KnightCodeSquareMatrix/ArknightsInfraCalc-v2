# Solve-Failure Evidence Ledger（求解失败证据台账）

> 文档角色：evidence
> 生命周期状态：current
> 当前真源：docs/质量与维护/QUALITY_AND_AUDIT.md
> 摘要：登记无用户反馈的求解失败证据族（`evidence/<日期>-<错误族>/raw-runs/`）。

与 `feedback/TRACKING.md` 分工：反馈台账记用户反馈；本台账记 solver 自身失败族（无反馈入口的失败 `serve` 运行）。

## 处置规则

1. 每族先本地回放：`evidence/<族>/replay.sh target/release/infra-cli <outdir>`，确认 `ok=false` 可复现。
2. 定位到最小层：CLI / 排班 / 候选 / 机制 / 数据；改代码前把复现命令记录到本行。
3. 状态与反馈台账同词表：`intake` / `reproduced` / `localized` / `fixing` / `closed` / `regressed` / `blocked`。
4. 新族出现时在下方「族清单」加一行标 `intake`；修复验证后更新为 `closed` 或 `regressed`。
5. `evidence/` 含真实练度盒与运行日志，保持 untracked；`raw-runs` 材料视为原始证据不改写。

## 族清单（人工维护）

| 族目录 | 状态 | 根因 / 处置 | 复现与证据 |
|--------|------|-------------|-----------|
| `2026-09-02-aic-plan-3004` | intake | 二班 `main_backup_12_12` no-solution；旧生产构建 `9dd38385…` 产生，最新构建回放仍复现 | `evidence/2026-09-02-aic-plan-3004/replay.sh` |
| `2026-09-02-belongs-to-multiple-teams` | intake | 二班 `main_backup_12_12` 多队冲突；构建 `509f8ac8…` 产生，本地 5/5 复现 | `evidence/2026-09-02-belongs-to-multiple-teams/replay.sh` |

## 自动捕获索引

<!-- BEGIN GENERATED SOLVE-FAILURE EVIDENCE -->
| 族目录 | 运行数 | 首错摘要 | CLI 指纹(8hex) |
|--------|-------:|---------|----------------|
| `2026-09-02-aic-plan-3004` | 59 | no solution under current-main complete-state-disjoint backup po | 43c3c85e, 9dd38385 |
| `2026-09-02-belongs-to-multiple-teams` | 5 | shift 1 operator 莱欧斯 belongs to multiple teams | 509f8ac8 |
<!-- END GENERATED SOLVE-FAILURE EVIDENCE -->
