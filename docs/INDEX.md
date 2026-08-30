# 文档入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md
> 摘要：按六类读者任务提供唯一文档总导航和生成索引

> 实现快照：Current
> 读者：玩家、策略作者、开发者、维护者、AI Agent
> 本文负责：在文档位置未知时提供路由；不定义业务公式，也不是每个任务的无条件首读
> 业务真源：用户当前裁决与对应领域 canonical Markdown

本页是 `docs/` 内唯一的总导航。每份文档仍只承担一种主要职责：教程带读者完成任务，说明页解释概念，规范页裁决领域语义，参考页记录接口或当前实现，治理页约束协作。不要按目录顺序通读，也不要用 TODO、ADR 或历史快照解释当前运行行为。

## 1. 开始

首次接触项目时，按目标选择一条最短路径：

| 读者 / 目标 | 建议入口 |
|---|---|
| 想先知道项目解决什么 | [项目首页](../README.md) → [项目总览](OVERVIEW.md) |
| 想亲手跑出一套方案 | [第一次完整运行：243 全精二方案](EXAMPLES/243_FULL_E2.md) |
| 懂基建策略，不关心代码 | [给策略作者的运行说明](GONGSUN_RUNTIME_OVERVIEW.md) → [术语表](GLOSSARY.md) |
| 想接入 CLI、Worker 或前端 | [infra-cli](INFRA_CLI.md) → [前端接口](FRONTEND_CLI.md) |
| 想沿一次请求理解代码 | [架构导览](ARCHITECTURE_TOUR.md) → [项目地图](PROJECT_MAP.md) |
| 想贡献代码或文档 | [贡献指南](../CONTRIBUTING.md) |

这些入口是读者快捷方式，不产生第二份事实 owner。遇到行为、接口或模块细节时，继续进入下列对应分区。

## 2. 使用与集成

| 任务 | 主文档 | 该页负责 |
|---|---|---|
| 运行完整默认方案 | [243 首次教程](EXAMPLES/243_FULL_E2.md) | 从命令到 stdout、profile 和 MAA 的完整操作 |
| 使用命令行 | [infra-cli 模块职责](INFRA_CLI.md) | 命令入口、参数、输出和 core/CLI 边界 |
| 接入 Worker / BFF | [前端对接说明](FRONTEND_CLI.md) | `plan.compute` 请求响应与兼容契约 |
| 部署长驻服务 | [前端 serve 指南](FRONTEND_SERVE_GUIDE.md) | 启动、探活、超时和部署操作 |
| 构建发布包 | [release README](../release/README.md) | 发布目录、构建脚本和发布夹具 |
| 更新 quality-check CLI | [quality-check CLI 更新规则](QUALITY_CHECK_CLI_UPDATE.md) | 构建、验证、上传和远端 bundle 验收 |
| 更新 dev 求解器 | [更新服务器求解器（dev 环境）](UPDATE_SOLVER.md) | 构建、推送、健康检查和自动回滚 |
| 使用练卡推荐 | [练卡推荐领域索引](练卡推荐相关/README.md) | 玩家产品语义、结构化输出与旧 v2 诊断边界 |

命令的当前参数以生成/runtime help 和 [INFRA_CLI.md](INFRA_CLI.md) 为准；前端不得从示例输出反推长期 wire schema。

## 3. 概念与能力

| 想理解 | 阅读 |
|---|---|
| 输入、输出和端到端求解模型 | [项目总览](OVERVIEW.md) |
| 对策略作者可见的运行过程 | [给策略作者的运行说明](GONGSUN_RUNTIME_OVERVIEW.md) |
| Rule、System、Plan、Team、Shift 等词汇 | [术语表](GLOSSARY.md) |
| 当前体系能力与各体系 owner | [体系导航](SYSTEM_CHAINS.md) |
| 已建模干员和数据覆盖 | [已建模干员](MODELLED_OPERATORS.md) |
| 当前 ABC、二班和具名四班实现 | [排班轮换](SCHEDULE_ROTATION.md) |

项目当前是领域约束下的分阶段、逐房搜索，不宣称全基建连续时间或整数规划意义上的全局最优。保证等级、性能边界和证据要求见 [质量与审计](QUALITY_AND_AUDIT.md)。

## 4. 领域规范

业务语义优先级唯一见根 [AGENTS.md 的“真源与任务路由”](../AGENTS.md#2-真源与任务路由)。下表按问题聚合入口，完整且机械校验的领域键 owner 表紧随其后。

| 主题 | 规范与当前参考 |
|---|---|
| 机制与数值 | [EffectAtom](EFFECT_ATOM_DESIGN.md)、[效率模型](EFFICIENCY_MODEL.md)、[评分口径](SCORING_MODEL.md) |
| 设施与单班编制 | [制造站](MANUFACTURE_STATUS.md)、[控制中枢](CONTROL_CENTER_ASSIGNMENT.md)、[全基建编制](BASE_ASSIGNMENT.md)、[编排层](ORCHESTRATION_LAYER.md) |
| 跨设施与成套体系 | [体系导航](SYSTEM_CHAINS.md) 及其中链接的各体系 canonical |
| 排班与换班 | [排班模式](排班模式.md)、[定时换班](定时换班.md)、[菲亚梅塔使用规则](菲亚梅塔使用规则.md) |
| 对外接口 | [infra-cli](INFRA_CLI.md)、[前端接口](FRONTEND_CLI.md)、[前端效率字段迁移说明](前端效率字段迁移说明.md) |
| 练卡推荐 | [练卡推荐领域索引](练卡推荐相关/README.md) |

### 完整 canonical owner 表

以下表格由 `python3 scripts/codex/docs_inventory.py --write-indexes` 根据文档 metadata 生成；不得手工修改生成标记内的行。

<!-- BEGIN GENERATED CANONICAL -->
| 领域键 | 权威入口 |
|---|---|
| `advice.training` | [练卡推荐玩家输出口径](练卡推荐相关/玩家输出口径.md) |
| `advice.training.combinations` | [练卡推荐组合知识库](练卡推荐相关/组合知识库.md) |
| `advice.training.newbie` | [新手基建必练与必收集名单](练卡推荐相关/新手必练与必收集名单.md) |
| `advice.training.output` | [练卡推荐玩家输出口径](练卡推荐相关/玩家输出口径.md) |
| `advice.training.standalone` | [高效率散件与搓玉名单](练卡推荐相关/高效率散件与搓玉名单.md) |
| `architecture.cli` | [infra-cli 模块职责](INFRA_CLI.md) |
| `architecture.orchestration` | [编排层重构路线图（Orchestration Layer）](ORCHESTRATION_LAYER.md) |
| `docs.lifecycle` | [文档生命周期](文档生命周期.md) |
| `facility.control-assignment` | [控制中枢排班规则](CONTROL_CENTER_ASSIGNMENT.md) |
| `facility.manufacture` | [制造站域状态](MANUFACTURE_STATUS.md) |
| `interface.frontend-cli` | [infra-cli + Layout 生成器 — 前端对接说明](FRONTEND_CLI.md) |
| `layout.assignment` | [全基建进驻编制（宏观排班）](BASE_ASSIGNMENT.md) |
| `mechanics.effect-atom` | [EffectAtom 设计文档](EFFECT_ATOM_DESIGN.md) |
| `production.drones` | [无人机生成、分配与产出结算](DRONES.md) |
| `schedule.fiammetta` | [菲亚梅塔使用规则](菲亚梅塔使用规则.md) |
| `schedule.mode` | [排班模式](排班模式.md) |
| `schedule.timed` | [定时换班（公孙长乐 × InfraCalc）](定时换班.md) |
| `scoring.efficiency` | [直接效率与整数结算架构](EFFICIENCY_MODEL.md) |
| `scoring.policy` | [评分口径审计](SCORING_MODEL.md) |
| `system.automation-group` | [自动化组体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/AUTOMATION_GROUP_CHAIN.md) |
| `system.fireworks` | [人间烟火排班规则](FIREWORKS.md) |
| `system.red-pine-forest` | [红松林体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/RED_PINE_FOREST_CHAIN.md) |
| `system.rhine-lab` | [莱茵生命体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/RHINE_LAB_CHAIN.md) |
| `system.rosemary-perception` | [迷迭香感知链体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md) |
| `workflow.maintenance` | [Debug 与一致性修复指南](MAINTENANCE_MODE.md) |
| `workflow.quality` | [质量、求解保证与验证证据总则](QUALITY_AND_AUDIT.md) |
| `workflow.system-audit` | [公孙长乐体系逐项审计与修复工作流](SYSTEM_AUDIT_WORKFLOW.md) |
<!-- END GENERATED CANONICAL -->

实现地图、汇总和示例不在 canonical 表重复登记。当前 ABC 实现见 [SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md)，体系导航见 [SYSTEM_CHAINS.md](SYSTEM_CHAINS.md)，干员建模覆盖见 [MODELLED_OPERATORS.md](MODELLED_OPERATORS.md)。

业务裁决与实现事实的优先级唯一见根 [AGENTS.md 的“真源与任务路由”](../AGENTS.md#2-真源与任务路由)。本节只把领域键路由到唯一 canonical owner；实现数据、测试、fixture 和历史输出不能反向裁决业务语义。

## 5. 技术参考

| 需要查询 | 单一入口 |
|---|---|
| 一次请求经过哪些模块 | [架构导览](ARCHITECTURE_TOUR.md) |
| 文件、模块、数据和命令 owner | [项目地图](PROJECT_MAP.md) |
| 高频修复先查哪里 | [当前实现状态入口](CURRENT_STATUS.md) |
| 调查 operbox 求解错误 | [求解错误分析报告](求解错误分析报告.md) |
| 贸易解释器、shortcut、跨设施等内部地图 | [内部实现索引](INTERNAL/README.md) |
| Rust 公共 API | `cargo doc -p infra-core --open`；crate landing 位于 [`crates/infra-core/src/lib.rs`](../crates/infra-core/src/lib.rs) |
| 性能模型、Bake 和候选边界 | [性能工程](PERFORMANCE_ENGINEERING.md) |
| 制造 `full_pool` / standalone 从哪里落到代码 | [制造站](MANUFACTURE_STATUS.md) → [项目地图 Owner 查询](PROJECT_MAP.md#常见-owner-查询) |
| 动态 producer admission / dependency / 同班从哪里落到代码 | [控制中枢](CONTROL_CENTER_ASSIGNMENT.md) → [项目地图 Owner 查询](PROJECT_MAP.md#常见-owner-查询) |
| 办公室 / 会客室当前出口与未来 `plan.compute` 扩展 | [前端接口](FRONTEND_CLI.md) → [项目地图 Owner 查询](PROJECT_MAP.md#常见-owner-查询) |

`PROJECT_MAP.md` 只回答“代码或数据在哪里”，rustdoc 只回答 public API；二者都不裁决领域行为。

### 架构决策

ADR 保存长期决策及理由，不声明当前实现已经完成。当前能力仍以 canonical、current reference 和代码为准。下表由文档 metadata 生成。

<!-- BEGIN GENERATED DECISIONS -->
| 决策 | 状态 | 摘要 |
|---|---|---|
| [ADR 0007: 回退恢复后的中枢与二发电事实边界](ADR/0007-regression-recovery-control-and-two-power-boundaries.md) | `accepted` | 恢复历史回退后中枢候选、动态 producer 与二发电宿舍数值的责任边界 |
| [ADR 0008: 办公室连续工作时长预算与通用轮换准入](ADR/0008-office-continuous-work-budget.md) | `proposed` | 规定办公室轮换按实际心情消耗计算连续工作时长，不按 profile 或干员姓名写死换班规则 |
| [ADR 0009: 无人机效率转化与产出核算](ADR/0009-drone-efficiency-conversion-and-production-accounting.md) | `accepted` | 记录发电效率、无人机库存、目标分配与制造贸易额外产出的统一裁决 |
| [ADR 0010: 菲亚梅塔固定策略分支与完整排班重算接入](ADR/0010-fiammetta-fixed-branches-full-recompute.md) | `proposed` | 菲亚梅塔作为阶段间固定策略分支选择器，N/A/B 三候选各自从同一 Box 完整重算整个排班；候选不预排锁房，靠 L1 shortcut 自然选中 + trade_core 偏好改写上班优先级，菲亚对固定两人各换一次心情，按完整周期收益选 winner |
<!-- END GENERATED DECISIONS -->

## 6. 开发与项目治理

| 工作 | 入口 |
|---|---|
| 人类贡献流程 | [CONTRIBUTING.md](../CONTRIBUTING.md) |
| Agent 任务分类、真源顺序和硬门禁 | [AGENTS.md](../AGENTS.md) |
| Debug 与一致性修复 | [维护指南](MAINTENANCE_MODE.md) |
| 正确性、求解保证和证据半径 | [质量与审计](QUALITY_AND_AUDIT.md) |
| 性能改动 | [性能工程](PERFORMANCE_ENGINEERING.md) |
| 严格体系审计 | [体系审计工作流](SYSTEM_AUDIT_WORKFLOW.md) |
| 文档角色、状态、owner 和关闭事务 | [文档生命周期](文档生命周期.md) |
| 可复现命令证据 | [Evidence 工具](../scripts/codex/README.md) |

Agent 的任务分类、Skill 选择和真源顺序只由根 `AGENTS.md` 负责；本文不维护第二份分类表，也不直接路由项目 Skills。

当前开放工作只看生成的 [TODO 索引](TODO/README.md)；只有被当前任务明确恢复后才执行。完成、被替代和历史材料见 [ARCHIVE](ARCHIVE/README.md)，它们只用于追溯。根 `plans/` 不再承载 Markdown。

一项业务规则只保留一个 canonical 定义，其他页面只做解释或链接。新增命令、性能数字和观察结果必须有可复现入口；易变化内容标注日期或生成来源。生成表使用 `python3 scripts/codex/docs_inventory.py --write-indexes` 更新，不手工修改标记内的行。
