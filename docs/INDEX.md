# Agent 文档总入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md；docs/agent-index/README.md
> 摘要：按症状、领域、代码、协议、活动和历史六类已知线索定位当前文档 owner

本文只负责文档导航。任务分类、真源顺序、硬门禁和 Skill 选择唯一由根 [`AGENTS.md`](../AGENTS.md) 负责；本文不维护第二份分类表。领域事实回到 canonical，当前实现回到代码锚点，历史计划不能解释当前行为。

## 1. 先确定检索路径

| 你已知什么 | 首选入口 | 适用情况 |
|---|---|---|
| 现象、报错或 bundle | [Agent 路由](生成索引/Agent路由.md) | 按症状进入调用链、模块、机制与验证 |
| 问题属于哪一类 | 本文第 2 节的七个领域入口 | 先看领域规范，再进入具体 owner |
| 函数、模块、命令或数据文件 | [项目地图](PROJECT_MAP.md) | 查稳定代码 owner 和入口 |
| CLI、Worker、MAA、wire schema 或字段 | 本文第 3 节 | 查接口合同、错误和输出消费者 |
| 当前是否有开放工作 | [TODO 总板](TODO/README.md) | 只看已登记活动项，不把历史 TODO 当队列 |
| 旧方案、完成记录或被替代设计 | [ARCHIVE](ARCHIVE/README.md) 与下方 ADR 表 | 追溯决策与历史，不覆盖 current owner |
| 只知道模糊概念或关键词 | `python3 scripts/codex/query_project_knowledge.py --query '<问题>'` | 关键词检索只负责导航，结果必须回 owner 核对 |
| 需要浏览全部页面 | [生成文档目录](生成索引/文档目录.md) | 按目录查看角色、状态和摘要，机械查漏 |

机器可读 route 注册表是 [`docs/agent-index/routes.json`](agent-index/routes.json)。如果症状路由与当前代码不一致，先以入口、代码和真实输出复核，再修正 route；不要从旧调用链猜。

## 2. 按问题进入领域

| 领域 | 从这里开始 | 主要问题 |
|---|---|---|
| 入门与术语 | [指南](指南/README.md) | 项目范围、架构导览、当前状态、术语和知识库用法 |
| 求解架构 | [架构](架构/README.md) | Plan、execute、fill、resolve、体系链路和状态责任 |
| 游戏机制与运行时映射 | [机制](机制/README.md) | 中枢、无人机、制造、效率、评分、EffectAtom 和技能映射 |
| 排班轮换 | [排班](排班/README.md) | profile、班次、菲亚梅塔、源石优先、bind 和跨班状态 |
| 接入与前端 | [接入与前端](接入与前端/README.md) | CLI、Worker、前端字段、serve 和部署验收 |
| 质量与维护 | [质量与维护](质量与维护/README.md) | 错误归因、质量门禁、性能、Bake 和体系审计 |
| 产品与训练 | [产品与训练](产品与训练/README.md) | 练卡输出、组合知识、名单和自然语言报告 |

以下 canonical 注册表用于从领域键回到唯一 owner。一个领域键只能有一个 canonical；页面之间的导航关系不等于事实所有权。

<!-- BEGIN GENERATED CANONICAL -->
| 领域键 | 权威入口 |
|---|---|
| `advice.training` | [练卡推荐玩家输出口径](产品与训练/玩家输出口径.md) |
| `advice.training.combinations` | [练卡推荐组合知识库](产品与训练/组合知识库.md) |
| `advice.training.newbie` | [新手基建必练与必收集名单](产品与训练/新手必练与必收集名单.md) |
| `advice.training.output` | [练卡推荐玩家输出口径](产品与训练/玩家输出口径.md) |
| `advice.training.standalone` | [高效率散件与搓玉名单](产品与训练/高效率散件与搓玉名单.md) |
| `architecture.cli` | [infra-cli 模块职责与命令地图](接入与前端/INFRA_CLI.md) |
| `docs.knowledge-bridge` | [游戏机制知识库引用合同](机制/知识库引用.md) |
| `docs.lifecycle` | [文档生命周期与 Agent 维护规则](文档生命周期.md) |
| `facility.control-assignment` | [控制中枢编制与效果账本](机制/CONTROL_CENTER_ASSIGNMENT.md) |
| `facility.manufacture` | [制造站求解状态](机制/MANUFACTURE_STATUS.md) |
| `interface.cli-worker` | [CLI 与 Worker 接口入口](接口/CLI与Worker.md) |
| `interface.errors` | [接口错误与诊断](接口/错误与诊断.md) |
| `interface.frontend-cli` | [前端与 `plan.compute` 对接](接入与前端/FRONTEND_CLI.md) |
| `interface.input-output` | [输入、输出与兼容契约](接口/输入输出与兼容.md) |
| `mechanics.effect-atom` | [EffectAtom 机制解释分层](机制/EFFECT_ATOM_DESIGN.md) |
| `production.drones` | [发电与无人机结算](机制/DRONES.md) |
| `schedule.auto_rotation` | [自动轮换](排班/自动轮换.md) |
| `schedule.fiammetta` | [菲亚梅塔使用规则](排班/菲亚梅塔使用规则.md) |
| `schedule.fiammetta-candidates` | [菲亚梅塔候选分支适配层设计](排班/菲亚梅塔候选分支适配层设计.md) |
| `schedule.mode` | [排班模式](排班/排班模式.md) |
| `schedule.timed` | [定时换班](排班/定时换班.md) |
| `schedule.timed.originium-pair` | [泡泡火神源石优先](排班/泡泡火神源石优先.md) |
| `scoring.efficiency` | [直接效率与整数结算](机制/EFFICIENCY_MODEL.md) |
| `scoring.policy` | [搜索评分与排序策略](机制/SCORING_MODEL.md) |
| `system.automation-group` | [自动化组体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/AUTOMATION_GROUP_CHAIN.md) |
| `system.red-pine-forest` | [红松林体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/RED_PINE_FOREST_CHAIN.md) |
| `system.rhine-lab` | [莱茵生命体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/RHINE_LAB_CHAIN.md) |
| `system.rosemary-perception` | [迷迭香感知链体系论证（公孙长乐 × InfraCalc）](公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md) |
| `workflow.maintenance` | [Debug 与一致性修复指南](质量与维护/MAINTENANCE_MODE.md) |
| `workflow.quality` | [质量、求解保证与验证证据总则](质量与维护/QUALITY_AND_AUDIT.md) |
| `workflow.system-audit` | [公孙长乐体系逐项审计与修复工作流](质量与维护/SYSTEM_AUDIT_WORKFLOW.md) |
<!-- END GENERATED CANONICAL -->

## 3. 按代码或协议进入

| 需要 | 首入口 | 继续阅读 |
|---|---|---|
| Rust 模块、命令、数据文件和调用链 | [项目地图](PROJECT_MAP.md) | 对应 `docs/模块/`、`docs/调用链/` 或 owner canonical |
| 生成模块与 CLI 命令地图 | [生成代码与文档地图](生成索引/代码与文档地图.md) | [Agent 路由](生成索引/Agent路由.md) |
| 第一次从夹具完整运行 `plan` | [243 全精二完整示例](EXAMPLES/243_FULL_E2.md) | [输入输出与兼容](接口/输入输出与兼容.md)、[debug 与验证](调用链/debug与验证.md) |
| Rust 内部注册表、解释器和符号级附录 | [内部实现细节](INTERNAL/README.md) | 对应 `docs/模块/` 的稳定行为 owner 与 `docs/调用链/` |
| CLI 与 Worker wire contract | [CLI 与 Worker](接口/CLI与Worker.md) | [输入输出与兼容](接口/输入输出与兼容.md) |
| 错误、bundle、反馈和复现 | [错误与诊断](接口/错误与诊断.md) | [debug 与验证](调用链/debug与验证.md)、[维护指南](质量与维护/MAINTENANCE_MODE.md) |
| MAA 导出或岗位投影 | [MAA 导出](调用链/MAA导出.md) | [导出与岗位状态](模块/导出与岗位状态.md) |
| 公孙长乐体系链、anchor 与 producer/consumer | [体系链路索引](架构/SYSTEM_CHAINS.md) | `docs/公孙长乐的体系分析文档/` 中的四份体系论证 |
| 最小请求、返回字段和前端读取样例 | [前端 `plan.compute` 对接](接入与前端/FRONTEND_CLI.md) | [前端效率字段迁移说明](接入与前端/前端效率字段迁移说明.md) |
| 外部机制原文 | [知识库引用合同](机制/知识库引用.md) | 固定 commit 的外部机制知识库正文 |

## 4. 当前工作与历史

[TODO 总板](TODO/README.md) 只登记 `active-change`，并按五个工作流组织详情：

| 工作流 | 何时进入 |
|---|---|
| [核心求解](TODO/核心求解.md) | 候选、机制解释、Plan admission、跨设施状态和求解质量 |
| [排班轮换](TODO/排班轮换.md) | profile、Team/Shift、事件、休息、回岗和跨班状态 |
| [接口产品](TODO/接口产品.md) | Worker、CLI、前端、训练室输出和网站产品结构 |
| [性能质量](TODO/性能质量.md) | 正确性归因、搜索性能、Bake、cache 和验证 |
| [文档协作](TODO/文档协作.md) | Agent 规则、文档 owner、导航和机械门禁 |

ADR 记录长期决策；[ARCHIVE](ARCHIVE/README.md) 保存已完成、被替代或仅供追溯的材料。两者都不能覆盖用户当前裁决或领域 canonical。

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
| [ADR 0014: 四赤金同场择优与自动轮换入口的实现路线](ADR/0014-four-gold-comparison-and-auto-rotation-roadmap.md) | `proposed` | 评估十月排班剩余两项（四赤金完整候选比较、自动轮换入口）的实现难度，列出必须由用户裁决的事项，并给出按单一意图拆分的修改路线 |
| [ADR 0015：挂件统一选入与开关边界](ADR/0015-挂件统一选入与开关边界.md) | `accepted` | 普通挂件复用既有设施搜索决定受益者入选，由统一执行者提交或回退，不新增全域收益比较 |
| [ADR 0016：怪猎低档协同与中枢光环选入](ADR/0016-怪猎低档协同与中枢光环选入.md) | `accepted` | 怪猎沿既有优先级串行选入，二人纯光环路径降至感知与龙门之后，不新增候选竞争或整体收益比较 |
| [ADR 0017：文档信息架构与分工作流 TODO](ADR/0017-document-information-architecture.md) | `accepted` | 以三个根 hub、七个领域入口、五条 TODO 工作流和生成式全量目录建立可预测的文档检索路径 |
<!-- END GENERATED DECISIONS -->

## 5. 文档维护与验证

| 需要 | 入口 |
|---|---|
| 角色、状态、关闭与归档规则 | [文档生命周期](文档生命周期.md) |
| 术语和替代词 | [术语表](指南/GLOSSARY.md) |
| Agent 知识检索、缓存和回取 | [Agent 知识库](指南/Agent知识库.md) |
| 删除旧实现前的检查路径 | [检查与删减导航](指南/检查与删减导航.md) |
| route、索引和代码地图生成 | [agent-index 说明](agent-index/README.md) |

维护文档后运行：

```bash
python3 scripts/codex/docs_inventory.py --check
python3 scripts/codex/generate_agent_index.py --check
python3 scripts/codex/check_repository_facts.py
python3 scripts/codex/query_project_knowledge.py --check --no-cache
```
