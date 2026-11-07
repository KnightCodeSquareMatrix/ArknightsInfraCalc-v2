# 机制入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/机制/知识库引用.md；docs/机制/EFFECT_ATOM_DESIGN.md；docs/机制/SCORING_MODEL.md
> 摘要：从游戏机制、运行时映射和求解策略三层定位当前机制 owner

## 阅读顺序

1. 先看 [知识库引用合同](知识库引用.md)，确认游戏机制原文的固定版本和引用边界。
2. 再看 [EffectAtom 分层](EFFECT_ATOM_DESIGN.md)，区分 L1 解释、L2 领域机制和 L3 实际组合结算。
3. 按问题进入中枢、无人机、制造、效率或评分 owner。
4. 需要代码定位时转到 [项目地图](../PROJECT_MAP.md) 或 [Agent 路由](../生成索引/Agent路由.md)。

| 问题 | 当前 owner |
|---|---|
| 中枢取最高、anchor、EffectLedger | [控制中枢编制与效果账本](CONTROL_CENTER_ASSIGNMENT.md) |
| 无人机库存、效率转化和产出 | [无人机与额外产出](DRONES.md) |
| 效率整数、阶段和单位 | [效率模型](EFFICIENCY_MODEL.md) |
| 制造配方、时间与仓储 | [制造站求解状态](MANUFACTURE_STATUS.md) |
| 搜索排序、剪枝和保证等级 | [评分模型](SCORING_MODEL.md) |
| 已建模与待建模能力 | [运行时已建模干员](MODELLED_OPERATORS.md)、[干员建模缺口](需要完成的干员建模.md) |

开放机制映射、候选准入与求解质量工作从 [核心求解工作流](../TODO/核心求解.md) 进入；提案不能覆盖当前 canonical。

## 本领域全部文档

<!-- BEGIN GENERATED DOMAIN DOCUMENTS -->
| 文档 | 角色 | 状态 | 摘要 |
|---|---|---|---|
| [控制中枢编制与效果账本](CONTROL_CENTER_ASSIGNMENT.md) | `canonical` | `current` | 说明控制中枢的 required anchor、顺序选位、EffectLedger 和全局注入责任边界。 |
| [发电与无人机结算](DRONES.md) | `canonical` | `current` | 说明发电效率、无人机库存和排班/导出动作之间的结算边界。 |
| [EffectAtom 机制解释分层](EFFECT_ATOM_DESIGN.md) | `canonical` | `current` | 定义 EffectAtom 的 L1/L2/L3 分层、phase/scope 语义和机制知识库的映射边界。 |
| [直接效率与整数结算](EFFICIENCY_MODEL.md) | `canonical` | `current` | 裁决千分整数效率、各生产域结算和排班输出字段的统一口径。 |
| [人间烟火排班链](FIREWORKS.md) | `current-reference` | `current` | 说明人间烟火 capability 在求解器中的 provider、consumer 和跨班绑定责任 |
| [制造站求解状态](MANUFACTURE_STATUS.md) | `canonical` | `current` | 裁决制造站输入、阶段解释、产线聚合、仓储和班次心情结算边界。 |
| [运行时已建模干员与能力覆盖](MODELLED_OPERATORS.md) | `current-reference` | `current` | 列出运行时已建模干员、能力覆盖和可核验边界，帮助区分实现事实与机制原文 |
| [搜索评分与排序策略](SCORING_MODEL.md) | `canonical` | `current` | 裁决各域排序、跨域具名 policy、剪枝保证等级和失败传播。 |
| [游戏机制知识库引用合同](知识库引用.md) | `canonical` | `current` | 规定求解器文档如何引用外部游戏机制知识库，以及机制事实与本项目实现的边界 |
| [干员建模缺口与核验入口](需要完成的干员建模.md) | `current-reference` | `current` | 汇总尚未建模或需复核的干员技能、证据入口与关闭条件 |
<!-- END GENERATED DOMAIN DOCUMENTS -->
