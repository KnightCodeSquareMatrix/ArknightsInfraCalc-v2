# Agent 文档入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md；docs/agent-index/README.md
> 摘要：从症状、入口、模块和机制引用定位完整求解器调用链

本文只负责路由。任务分类、硬门禁和 Skill 选择唯一由根 [`AGENTS.md`](../AGENTS.md) 负责；本文不维护第二份分类表，也不复制第二套规则。机器可读的 route 注册表是 [`docs/agent-index/routes.json`](agent-index/routes.json)。当前运行事实必须回到代码锚点和对应 owner，历史计划不能解释当前行为。

理解项目整体模型先读[项目逻辑内核](项目逻辑内核.md)；定位具体问题继续按下面路由读取。

自动轮换最新交付：[删减实施与速度验收](ARCHIVE/done/自动轮换删减实施与速度验收-20261030.md)：已删除禁用搜索与全站证明；保证取舍、实施清单、单次速度对照和最终复审入口均在报告中。前轮[主力优先修复](ARCHIVE/done/自动轮换主力优先与准入性能修复-20261030.md)、[意图与性能审计](ARCHIVE/audits/自动轮换意图与性能审计-20261030.md)保留追溯。

## 1. 从症状开始

先打开生成的 [Agent 路由](生成索引/Agent路由.md)，按症状找到链路 ID，再阅读链路页、模块页和验证入口。

| 症状 | 首入口 | 后续 |
|---|---|---|
| `plan`、Worker、profile、MAA 结果不一致 | [`CLI.PLAN`](生成索引/Agent路由.md) | [plan 请求](调用链/plan请求.md) → [CLI 与 Worker](接口/CLI与Worker.md) |
| 干员被抢、体系没激活、房间不满 | [`LAYOUT.SHIFT`](生成索引/Agent路由.md) | [跨设施状态刷新](调用链/跨设施状态刷新.md) → 候选与 Plan 模块 |
| 贸易效率、订单、shortcut 或单位产出错误 | [`SOLVER.TRADE_ROOM`](生成索引/Agent路由.md#solvertrade_room) | [技能到房间结算](调用链/技能到房间结算.md) → 贸易求解模块 |
| 制造配方、产线、仓储或效率错误 | [`SOLVER.MANUFACTURE_ROOM`](生成索引/Agent路由.md#solvermanufacture_room) | [技能到房间结算](调用链/技能到房间结算.md) → 制造求解模块 |
| 发电效率、无人机或电站人数错误 | [`SOLVER.POWER_ROOM`](生成索引/Agent路由.md#solverpower_room) | [技能到房间结算](调用链/技能到房间结算.md) → 心情与辅助设施 |
| 中枢、producer/consumer 或跨房间注入错误 | [`CONTROL.CROSS_FACILITY`](生成索引/Agent路由.md#controlcross_facility) | [跨设施状态刷新](调用链/跨设施状态刷新.md) → 中枢与跨设施状态 |
| 干员未进池、tier、设施技能或 buff_id 错误 | [`INPUT.SKILL_MAPPING`](生成索引/Agent路由.md#inputskill_mapping) | [技能到房间结算](调用链/技能到房间结算.md) → 输入与技能映射 |
| 候选未考虑、排序/剪枝或 Bake 异常 | [`SEARCH.BAKE`](生成索引/Agent路由.md#searchbake) | [候选搜索与 Bake](调用链/候选搜索与Bake.md) → 候选搜索模块 |
| ETA、宿舍、办公室或会客室异常 | [`MOOD.SUPPORT`](生成索引/Agent路由.md#moodsupport) | [跨设施状态刷新](调用链/跨设施状态刷新.md) → 心情与辅助设施 |
| 班次互斥、profile、休息或绑定错误；泡泡/火神未进首个源石房 | [`ROTATION.TIMED`](生成索引/Agent路由.md#rotationtimed) | [定时轮换](调用链/定时轮换.md) → [源石优先](泡泡火神源石优先.md) → 排班模块 |
| MAA 字段或房间与 assignment 不同 | [`EXPORT.MAA`](生成索引/Agent路由.md#exportmaa) | [MAA 导出](调用链/MAA导出.md) → 导出接口 |
| 线上反馈、debug bundle 或求解错误 | [`DIAGNOSTICS.DEBUG`](生成索引/Agent路由.md#diagnosticsdebug) | [debug 与验证](调用链/debug与验证.md) → 错误诊断接口 |
| 回归 case 失败或需要保留失败证据 | [`VERIFY.REGRESSION`](生成索引/Agent路由.md#verifyregression) | [debug 与验证](调用链/debug与验证.md) → 求解错误台账 |
| training_advice 或练卡组合说明异常 | [`ADVICE.TRAINING`](生成索引/Agent路由.md#advicetraining) | [plan 请求](调用链/plan请求.md) → 练卡推荐领域 |

## 2. 按入口阅读

| 入口 | 先读 | 代码起点 |
|---|---|---|
| `infra-cli plan` / `serve` | [CLI 与 Worker](接口/CLI与Worker.md) → [plan 请求](调用链/plan请求.md) | `crates/infra-cli/src/main.rs::run` → `commands/plan_compute.rs::compute_plan` |
| `layout test` / `layout eval` | [输入输出与兼容](接口/输入输出与兼容.md) → [跨设施状态刷新](调用链/跨设施状态刷新.md) | `crates/infra-cli/src/commands/layout.rs` |
| `layout team-rotation` | [排班与轮换](模块/排班与轮换.md) → [定时轮换](调用链/定时轮换.md) | `crates/infra-core/src/schedule/team_rotation.rs::schedule_timed_rotation_request` |
| `debug rerun` | [错误与诊断](接口/错误与诊断.md) → [debug 与验证](调用链/debug与验证.md) | `crates/infra-cli/src/commands/debug.rs::debug_cmd` |
| `bake` / `profile` / `verify` | [候选搜索与 Bake](模块/候选搜索与Bake.md) → [debug 与验证](调用链/debug与验证.md) | 对应 `crates/infra-cli/src/commands/*.rs` |

## 3. 按模块阅读

| 责任域 | 当前 owner |
|---|---|
| 输入、tier、`buff_id` 和池构建 | [输入与技能映射](模块/输入与技能映射.md) |
| 贸易站单房与 L1/L2/L3 | [贸易求解](模块/贸易求解.md) |
| 制造站配方、时间和产出 | [制造求解](模块/制造求解.md) |
| 中枢 ledger、全局资源和 resolve | [中枢与跨设施状态](模块/中枢与跨设施状态.md) |
| 候选、剪枝、standalone/full pool、Bake | [候选搜索与 Bake](模块/候选搜索与Bake.md) |
| 排班、班次、bind、心情与岗位 | [排班与轮换](模块/排班与轮换.md) |
| 导出、岗位状态与投影 | [导出与岗位状态](模块/导出与岗位状态.md) |

模块页只描述本项目当前行为和责任边界；游戏机制沿 [知识库引用合同](知识库引用.md) 进入对应章节。

## 4. 接口与验证

| 需要 | 入口 |
|---|---|
| CLI、Worker、协议和字段 | [CLI 与 Worker](接口/CLI与Worker.md)、[输入输出与兼容](接口/输入输出与兼容.md) |
| 错误、bundle、反馈和复现 | [错误与诊断](接口/错误与诊断.md)、[debug 与验证](调用链/debug与验证.md) |
| 正确性保证和证据 | [质量、求解保证与验证证据](QUALITY_AND_AUDIT.md)、[Evidence 工具](../scripts/codex/README.md) |
| 两班、三班、自动轮换耗时与热点 | [挂件修正后排班性能评估](ARCHIVE/done/排班性能评估-20261029.md)、[性能后续交接](TODO/性能优化交接.md#413-挂件修正后详细profile2026-10-29) |
| 生成模块和 route 索引 | [生成代码与文档地图](生成索引/代码与文档地图.md)、[路由注册表说明](agent-index/README.md) |
| 当前源码模块 owner | [项目地图](PROJECT_MAP.md) |

## 5. 外部机制与历史材料

- 游戏机制只通过 [知识库引用合同](知识库引用.md) 跳转到固定 commit 的知识库正文；草稿必须保留状态。
- 当前领域规范见本目录根页和模块页；代码、JSON、fixture、旧输出只证明当前实现，不反向裁决业务语义。
- 活动工作只看 [TODO 索引](TODO/README.md)；ADR 只记录持久决策；已完成、被替代和历史材料只看 [ARCHIVE](ARCHIVE/README.md)。

<!-- BEGIN GENERATED CANONICAL -->
| 领域键 | 权威入口 |
|---|---|
| `advice.training` | [练卡推荐玩家输出口径](练卡推荐相关/玩家输出口径.md) |
| `advice.training.combinations` | [练卡推荐组合知识库](练卡推荐相关/组合知识库.md) |
| `advice.training.newbie` | [新手基建必练与必收集名单](练卡推荐相关/新手必练与必收集名单.md) |
| `advice.training.output` | [练卡推荐玩家输出口径](练卡推荐相关/玩家输出口径.md) |
| `advice.training.standalone` | [高效率散件与搓玉名单](练卡推荐相关/高效率散件与搓玉名单.md) |
| `architecture.cli` | [infra-cli 模块职责与命令地图](INFRA_CLI.md) |
| `docs.knowledge-bridge` | [游戏机制知识库引用合同](知识库引用.md) |
| `docs.lifecycle` | [文档生命周期与 Agent 维护规则](文档生命周期.md) |
| `facility.control-assignment` | [控制中枢编制与效果账本](CONTROL_CENTER_ASSIGNMENT.md) |
| `facility.manufacture` | [制造站求解状态](MANUFACTURE_STATUS.md) |
| `interface.cli-worker` | [CLI 与 Worker 接口入口](接口/CLI与Worker.md) |
| `interface.errors` | [接口错误与诊断](接口/错误与诊断.md) |
| `interface.frontend-cli` | [前端与 `plan.compute` 对接](FRONTEND_CLI.md) |
| `interface.input-output` | [输入、输出与兼容契约](接口/输入输出与兼容.md) |
| `mechanics.effect-atom` | [EffectAtom 机制解释分层](EFFECT_ATOM_DESIGN.md) |
| `production.drones` | [发电与无人机结算](DRONES.md) |
| `schedule.auto_rotation` | [自动轮换](自动轮换.md) |
| `schedule.fiammetta` | [菲亚梅塔使用规则](菲亚梅塔使用规则.md) |
| `schedule.fiammetta-candidates` | [菲亚梅塔候选分支适配层设计](菲亚梅塔候选分支适配层设计.md) |
| `schedule.mode` | [排班模式](排班模式.md) |
| `schedule.timed` | [定时换班](定时换班.md) |
| `schedule.timed.originium-pair` | [泡泡火神源石优先](泡泡火神源石优先.md) |
| `scoring.efficiency` | [直接效率与整数结算](EFFICIENCY_MODEL.md) |
| `scoring.policy` | [搜索评分与排序策略](SCORING_MODEL.md) |
| `system.automation-group` | [自动化组体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/AUTOMATION_GROUP_CHAIN.md) |
| `system.red-pine-forest` | [红松林体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/RED_PINE_FOREST_CHAIN.md) |
| `system.rhine-lab` | [莱茵生命体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/RHINE_LAB_CHAIN.md) |
| `system.rosemary-perception` | [迷迭香感知链体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md) |
| `workflow.maintenance` | [Debug 与一致性修复指南](MAINTENANCE_MODE.md) |
| `workflow.quality` | [质量、求解保证与验证证据总则](QUALITY_AND_AUDIT.md) |
| `workflow.system-audit` | [公孙长乐体系逐项审计与修复工作流](SYSTEM_AUDIT_WORKFLOW.md) |
<!-- END GENERATED CANONICAL -->

<!-- BEGIN GENERATED DECISIONS -->
| 决策 | 状态 | 摘要 |
|---|---|---|
| [ADR 0007: 回退恢复后的中枢与二发电事实边界](ADR/0007-regression-recovery-control-and-two-power-boundaries.md) | `accepted` | 恢复历史回退后中枢候选、动态 producer 与二发电宿舍数值的责任边界 |
| [ADR 0008: 办公室连续工作时长预算与通用轮换准入](ADR/0008-office-continuous-work-budget.md) | `proposed` | 规定办公室轮换按实际心情消耗计算连续工作时长，不按 profile 或干员姓名写死换班规则 |
| [ADR 0009: 无人机效率转化与产出核算](ADR/0009-drone-efficiency-conversion-and-production-accounting.md) | `accepted` | 记录发电效率、无人机库存、目标分配与制造贸易额外产出的统一裁决 |
| [ADR 0010: 菲亚梅塔固定策略分支与完整排班重算接入](ADR/0010-fiammetta-fixed-branches-full-recompute.md) | `superseded` | 菲亚梅塔作为阶段间固定策略分支选择器，N/A/B 三候选各自从同一 Box 完整重算整个排班；候选不预排锁房，靠 L1 shortcut 自然选中 + trade_core 偏好改写上班优先级，菲亚对固定两人各换一次心情，按完整周期收益选 winner |
| [ADR 0011: 二/三班通用时长排班与泡泡火神全局强制](ADR/0011-generic-two-three-shift-rotation.md) | `accepted` | 为普通二班/三班增加输入时长并共用终态收尾，在全部该范围内强制泡泡/火神源石预占 |
| [ADR 0012: 十月排班需求的分项交付判定与关闭边界](ADR/0012-october-rotation-delivery-boundaries.md) | `accepted` | 记录六项十月排班需求在各自语义边界内的交付状态，不把局部实现或工作区草稿合并宣称为整体完成 |
| [ADR 0013: 四赤金作为 Trade Core 候选](ADR/0013-four-gold-trade-core-candidate.md) | `accepted` | 明确四赤金是 Trade Core 候选中的专用跨站方案，并规定其与普通 Trade Core 的比较边界 |
| [ADR 0014: 四赤金同场择优与自动轮换建议模式的实现路线](ADR/0014-four-gold-comparison-and-auto-rotation-roadmap.md) | `proposed` | 评估十月排班剩余两项（四赤金完整候选比较、自动轮换建议模式）的实现难度，列出必须由用户裁决的事项，并给出按单一意图拆分的修改路线 |
<!-- END GENERATED DECISIONS -->
