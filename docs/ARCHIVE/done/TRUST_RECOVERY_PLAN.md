# Feedback Maintenance Reference: Closure Audit, Repro, Trace, Regression

> 文档角色：archive
> 生命周期状态：completed
> 替代项：feedback/TRACKING.md；docs/质量与维护/MAINTENANCE_MODE.md
> 历史原因：生产反馈批次已关闭，维护入口已由 feedback ledger 和维护文档接管
> 快照日期：2026-07-18
> 摘要：保存生产反馈信任恢复和关闭审计的完成记录

> 历史原状态：maintenance-reference
> 启动日期：2026-07-03
> 替代范围：暂停 `QUALITY_90_TO_95_PLAN.md` 的大架构推进，先恢复结果可信度与排错速度。
> 收口修正：2026-07-03 用户确认 `feedback/` 中本批 bug 已修复；当前任务从“继续修反馈”切换为“固化关闭证据与防回归矩阵”。
> 维护修正：2026-07-03 用户确认项目已经足够好，进入正常维护期；本文不再是主动计划，只是维护参考。

## 0. 判断

当前主要风险不是推荐质量从 90 分到 95 分，而是：

> 结果错时，很难快速判断错在数据、机制、搜索、编排、排班、导出还是展示。

因此当前口径从“继续做候选架构”切换为“正常维护”。本批 `feedback/` bug 修复完成后，本文的作用是把关闭状态、修复证据和 smoke matrix 留在仓库里，避免后续 agent 误以为这些反馈仍是开放队列。

## 1. 非目标

本阶段不做：

- 不引入统一 `TeamCandidate` / `TeamColumn` 主架构；
- 不推进全局 selector、SystemRule registry、materialized candidate view；
- 不为了单个反馈泛化体系编排；
- 不自动把中文反馈或机制原文转成规则；
- 不追求全仓库重构或零 warning；
- 不扩大非目标：心情排班、宿管恢复、全基建连续时间最优化仍不做。

`QUALITY_90_TO_95_PLAN.md` 中已有的 trace-only pilot、机制审计和反馈材料可以作为证据使用，但不再作为默认建设路线。

## 2. 当前资产

| 资产 | 用途 |
|------|------|
| [../../feedback/](../../../feedback/) | 生产反馈原始证据 |
| [../../feedback/TRACKING.md](../../../feedback/TRACKING.md) | 反馈 ledger，记录状态、疑似层、下一步 |
| [../MAINTENANCE_MODE.md](../../质量与维护/MAINTENANCE_MODE.md) | 单个 bug 的复现、定位、修复流程 |
| [../PROJECT_MAP.md](../../PROJECT_MAP.md) | 代码边界与入口 |

## 3. 工作流

新反馈按同一条闭环处理：

1. `intake`：从 `issue.json` / `meta.json` / `debug-bundle.json` 记录反馈 id、房间、用户期望和线上命令。
2. `reproduced`：用本地 CLI 复现，优先复用 debug bundle 中的 layout 与 operbox。
3. `localized`：确认错层，只允许落在一个主层：CLI / layout / schedule / search / solver / mechanism / data / output。
4. `fixing`：只改确认错层，避免顺手重构。
5. `regressed`：补最小回归，优先 fixture / CSV / targeted test / debug bundle smoke。
6. `closed`：在 `feedback/TRACKING.md` 写清 commit、验证命令和确认层。

不能复现时标记 `blocked`，写明已跑命令和缺失材料，不猜公式。

本批历史反馈已经进入 closed / duplicate-covered 状态；不要再把它们当作默认待修队列。

## 4. Trace v0 原则

Trace v0 不是完整解释系统，只补关键边界的可观测点。

优先加这些小 trace：

| 层 | 需要看见什么 |
|----|--------------|
| layout/orchestrate | 哪个体系认领了哪些干员；谁因占用或缺条件被拒 |
| assign/fill | 房间为什么空置；候选池数量；最终 fallback 来源 |
| search | top candidate、被用户期望组合的 raw score、是否缺人 |
| control/global | 注入来源、同类效果是否取最高、重复项来源 |
| schedule/shift_bind | 固定组是否换站；换站原因 |
| output/serve | 前端看到的字段是否来自 core，还是展示层加工 |

Trace 默认应是 debug-only，不能改变推荐结果。

## 5. 防回归矩阵

本批反馈收口后，主要依靠以下矩阵守住回归：

```bash
cargo test -p infra-core --quiet
cargo run -q -p infra-cli -- verify --all
cargo run -q -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

重点覆盖：

| Family | Guard |
|--------|-------|
| 发电站候选不足 / 小车优先级 | power assignment / search targeted tests |
| 贸易 Vina / 巫恋换站 / U-Official | trade role and schedule binding tests |
| 制造红云克里斯汀 / 槐琥 / 清流温蒂冬时 | manufacture search tests and trace seed |
| 中枢重复 / filler 机会成本 | control layered-fill tests |
| 宿舍等级 / 无效宿舍锚点 | layout metadata and dorm semantics tests |

## 6. 维护标准

正常维护期的健康标准不是“没有 bug”，而是：

- 每条生产反馈都在 `feedback/TRACKING.md` 有 closed / duplicate-covered 状态；
- 已知修复提交和测试入口能从 tracking 找到；
- smoke matrix 写在 tracking 和本文档里；
- 后续 agent 不需要全仓库通读，就能从 feedback id 进入对应层；
- `QUALITY_90_TO_95_PLAN.md` 不再被默认当作下一步实现路线。
