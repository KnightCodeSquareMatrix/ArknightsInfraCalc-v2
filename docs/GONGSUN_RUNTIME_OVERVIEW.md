# 给策略作者的项目运行说明

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/EFFECT_ATOM_DESIGN.md；docs/BASE_ASSIGNMENT.md；docs/ORCHESTRATION_LAYER.md；docs/SCHEDULE_ROTATION.md
> 摘要：面向基建策略作者解释当前求解流程和策略输入边界

> 面向：懂《明日方舟》基建体系、但不需要阅读代码的人。
> 目标：解释项目怎样把练度、布局和领域规则变成编制与轮换，以及策略知识应该描述什么。

## 1. 这个程序解决什么问题

输入是玩家练度盒、基建布局和已经确认的领域规则。输出不只是某间贸易站的三人组，而是同一班内所有房间的实际进驻、各生产域效率、跨设施关系，以及 timed rotation 中每班谁上岗、谁休息。

程序必须同时处理：

- 干员是否拥有、技能是否已解锁；
- 房间类型、容量、等级、配方和订单；
- 同一干员不能同时占两个岗位；
- 同房 bond、required anchor、禁止同房和跨设施 producer/consumer；
- 中枢与全局资源怎样改变生产房上下文；
- Team/Shift 中已经入选成员的同上同下关系；
- profile、终端文本和 MAA 是否忠实消费同一次求解结果。

项目目标是在已确认约束下生成可解释、可复现的高效方案。当前单班编制是分阶段、逐房搜索，不宣称全基建全班次的无界全局最优。

## 2. 规则和数据的地位不同

业务语义按以下顺序判断：

1. 用户当前明确确认或纠正的口径；
2. 对应领域唯一 canonical Markdown；
3. 当前代码和生成 help 所证明的实现事实；
4. JSON、CSV、fixture、Bake、测试和旧输出等运行时载体或核对材料。

因此，数据文件可以承载规则，但不能反向裁决业务语义。

| 文件或文档 | 当前职责 |
|---|---|
| canonical Markdown | 定义技能、体系、排班或接口的不变量 |
| `operator_instances.json` | 干员、练度档、设施和 `buff_id` 的运行时归属 |
| `skill_table.json` | `buff_id -> SkillDef / EffectAtom[]` 的运行时机制表 |
| `orchestration_rules.json` | 声明式 Rule、alternative、role、relation 和 admission catalog |
| `base_systems.json` | 尚未迁移体系的 legacy compatibility registry |
| `producer_rules.json` | 动态 producer 的响应依赖和资源读取声明 |
| `trade_segments.json` / `trade_shortcuts.json` | 贸易链段、role fallback 和实际组合结算表 |
| `standalone_roster.json` | 具名 standalone 搜索边界；不裁剪普通排班制造 full pool |
| `MECHANICS_REGISTRY.csv` / `prts_*` | 外部原文快照、导入来源和历史核对材料 |

`MECHANICS_REGISTRY.csv` 不是仓库业务真源。技能原文、社区策略和当前实现发生分歧时，先回到对应 canonical 文档裁决，再更新运行时载体。

## 3. 一次方案怎样生成

当前主路径可以概括为：

```text
练度盒 + 布局 + runtime data
  -> 编译 Rule alternatives，并汇合 legacy registry
  -> build_plan：解析 fixed / bond / required anchor / bind
  -> execute_plan：提交已经完全解析的 placement
  -> 按阶段建立候选池并填充中枢、producer、发电、贸易、制造
  -> resolve：重建 workforce、全局资源和各房间输入
  -> timed rotation：按具名 profile 生成每个 Shift
  -> profile / 终端文本 / MAA / Worker 响应
```

这条路径把三种责任分开：

- **机制结算**回答“实际在房间里的这些人怎样生效”。
- **编排 admission**回答“哪些核心和关系必须进入合法方案”。
- **自然搜索**回答“剩余合法自由度里，哪些队友效率更高”。

shortcut 只结算实际组合，不能因为组合很强就负责把人塞进编制；`shift_bind` 只约束已经合法入选的成员怎样轮换，也不能代替 required anchor。

## 4. 机制怎样结算

普通技能尽量拆成声明式 `EffectAtom`，由 selector、action、condition、phase 和 scope 组成。L1 解释器只认 `buff_id`，不按干员中文名偷偷改公式。

贸易领域还包含两层具名机制：

- L2 处理赤金虚拟线、订单分布、违约、裁缝和单位产出等领域状态；
- L3 shortcut 按实际同房组合和 producer 前提匹配已确认规则。

制造和发电使用各自的求值与搜索路径，不照搬贸易假设。贸易、制造、发电分别保存直接效率，不合并成一个无法解释的匿名总分。

完整词汇见 [EffectAtom 设计](EFFECT_ATOM_DESIGN.md) 和 [效率模型](EFFICIENCY_MODEL.md)。

## 5. 策略关系应怎样描述

策略作者最重要的工作不是提供一张固定排班表，而是区分关系类型和自由度。

| 关系 | 应描述的内容 | 不应偷换成 |
|---|---|---|
| 全局资源 | 谁生产、谁读取、何时结算 | 固定三人组 |
| 跨设施关系 | producer/consumer、设施、是否要求同班 | 仅凭 tag 强制进编 |
| 同房 bond | 必须同房的核心、允许的第三人或备选 | 当前 top hit 的完整房间 |
| required anchor | 哪个核心在什么条件下必须进编、放到哪类房间 | shortcut 或 priority |
| role / fallback | 核心优先顺序和缺人时怎样降级 | 永远固定同一队友 |
| 自然效率关系 | 合法候选范围和评分口径 | 没有业务依据的 System |

例如，全局资源应先由实际在岗 producer 产生，再让 consumer 读取；它不自动意味着两人同房。一个强贸易核心可以要求进站，但其余队友仍应在当前上下文中自然搜索，除非 canonical 明确规定同房关系。

## 6. 候选、硬约束和 policy

硬约束负责排除非法状态，典型例子包括 required anchor、禁止同房、房间容量、一人一岗和明确的同班关系。它们不能依赖 `top_k`、tag、priority 或某次搜索刚好命中。

合法空间内的选择由各领域负责：

- 普通排班制造使用全部合法普通制造候选，不由 `standalone_roster.json` 裁剪；
- 贸易等适用领域可以使用具名 roster 或 role policy 缩小候选边界；
- 中枢和动态 producer 使用具名 policy 比较，不把局部 heuristic 冒充生产效率；
- 房间之间存在 workforce 依赖时，按 pipeline 顺序提交并重新 resolve。

任何缩池、剪枝、Bake 或 cache 都必须说明是安全缩减、heuristic、policy 还是 approximation。缓存不兼容时应回退 live solver，而不是悄悄改变语义。

## 7. 单班怎样变成轮换

`schedule_timed_rotation_request` 当前支持：

| profile | 形状 | 用途 |
|---|---|---|
| 默认 ABC | `12h + 6h + 6h` | alpha、beta 来自 peak 两半，gamma 是替补队 |
| 二班 | `12h + 12h` | 主力与替补两班 |
| 菲亚四班 | `8h + 8h + 4h + 4h` | 带菲亚事件边界的具名 profile |
| 深海四班 | `7h + 5h + 7h + 5h` | 深海体系具名 profile |

默认 ABC 只是 timed rotation 的一个 profile，不代表全部排班模式。Mower 动态换班和游戏内自动轮换有不同的触发与责任边界；模式规范见 [排班模式](排班模式.md)，当前实现见 [排班轮换](SCHEDULE_ROTATION.md)。

## 8. 怎样提供可执行的策略知识

建议每条策略至少回答：

| 字段 | 需要说明 |
|---|---|
| 名称与适用布局 | 体系/规则名称，适用于哪些房间、配方或订单 |
| admission | 何时必须启用、何时必须关闭 |
| 角色 | required、optional、producer、consumer、自然队友 |
| 关系与作用域 | 同房、跨站、全基建、同班或跨班 |
| fallback | 缺人、资源不足或布局不满足时怎样降级 |
| 互斥 | 禁止同房、禁止共存或资源冲突 |
| 评分 | 哪个具名效率或 policy 决定合法候选之间的顺序 |
| 验收 | 最小激活、拒绝边界和相邻反例 |

具体成员、房号、Shift 下标和某次全精二 top hit 只有在它们本身就是已确认不变量时才能成为规则。

## 9. 继续阅读

- 想跑一遍真实方案：[第一次完整运行：243 全精二方案](EXAMPLES/243_FULL_E2.md)。
- 想看完整端到端解释：[项目总览](OVERVIEW.md)。
- 想定义编排关系：[编排层](ORCHESTRATION_LAYER.md) 和 [全基建编制](BASE_ASSIGNMENT.md)。
- 想查具体体系：[体系导航](SYSTEM_CHAINS.md)。
- 想判断结果保证：[质量与审计](QUALITY_AND_AUDIT.md)。
- 想查代码与数据 owner：[项目地图](PROJECT_MAP.md)。
