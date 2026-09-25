# 基建体系链入口

> 文档角色：index-reference
> 生命周期状态：current
> 当前真源：docs/知识库引用.md
> 摘要：导航已注册体系 capability、Plan 和运行时 owner，不复制机制正文
> 代码范围：data/orchestration_rules.json、crates/infra-core/src/layout/orchestrate/、global_resource/
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：build_plan、execute_plan、AssignmentPlan.resolved_producer_dependencies
> 关联链路：docs/ORCHESTRATION_LAYER.md、docs/BASE_ASSIGNMENT.md、docs/FIREWORKS.md
> 核验基线：知识库 canonical 章节与 runtime capability 映射

本文只做导航，不复制机制、成员、倍率、优先级或历史路线图。每条机制先查隔壁知识库 canonical，再沿 runtime capability 和 Plan/Execute 定位代码。

| 链路 | 机制真源 | 实现入口 |
|---|---|---|
| 迷迭香/感知 | 隔壁知识库 canonical | data/orchestration_rules.json、layout/orchestrate/、global_resource/ |
| 自动化/制造 | 隔壁知识库 canonical | layout/orchestrate/、manufacture/、power/ |
| 红松林 | 隔壁知识库 canonical | layout/orchestrate/、manufacture/ |
| 莱茵 | 隔壁知识库 canonical | layout/orchestrate/、manufacture/、power/ |
| 人间烟火 | 隔壁知识库 canonical | trade/interpreter.rs、schedule/shift_bind.rs、FIREWORKS.md |
| 贸易 core/producer | 隔壁知识库 canonical | layout/assign/trade_fill.rs、trade/、layout/orchestrate/ |

统一排查顺序：知识库章节 → runtime capability → data rule/alternative → build_plan → execute_plan → facility fill/resolve_base → schedule shift_bind/rotation → export。规则与代码冲突时，先记录 source/version/gap 到 docs/知识库引用.md，再分别修桥接和实现文档。
