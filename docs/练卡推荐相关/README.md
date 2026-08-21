# 练卡推荐领域索引

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/练卡推荐相关/玩家输出口径.md
> 摘要：练卡推荐玩家业务真源入口、责任边界、读取顺序与实现差距

## 真源顺序

1. 用户当前明确裁决。
2. 本目录中与问题直接对应的 canonical（及已批 [组合成员练度审阅稿](组合成员练度审阅稿.md)）。
3. 代码与 CLI 证明**当前实现事实**（可能仍落后于本目录）。
4. `data/training_recommendations.json`、旧夹具、渲染稿只用于核对**旧 v2 机器诊断**，不得反向裁决本目录。

总领域键 `advice.training` 由 [玩家输出口径](玩家输出口径.md) 拥有；子域见下表。

**已替代的旧合同：** [练卡推荐规则_v2机器诊断合同](../ARCHIVE/superseded/练卡推荐规则_v2机器诊断合同.md)（原 `docs/练卡推荐规则.md`）。

## Canonical 文档

| 文档 | Owner |
| --- | --- |
| [玩家输出口径](玩家输出口径.md) | `advice.training` + `advice.training.output`：报告结构、≤10、C/T 裁决 |
| [新手必练与必收集名单](新手必练与必收集名单.md) | `advice.training.newbie` |
| [组合知识库](组合知识库.md) | `advice.training.combinations` |
| [高效率散件与搓玉名单](高效率散件与搓玉名单.md) | `advice.training.standalone` |

参考：[组合成员练度审阅稿](组合成员练度审阅稿.md)、[玩家报告实现计划](玩家报告实现计划.md)、[实现 TODO 提示](实现TODO提示.md)。

自然语言报告层设计见[自然语言推荐层设计](自然语言推荐层设计.md)。该文档记录已确认的 LLM、BFF、Redis、mapping 与降级边界；不改变当前结构化 `player` 输出契约。

实现交接与独立验收入口见[自然语言推荐层实现交接与验收](自然语言推荐层实现交接与验收.md)。

凯尔希叙事见组合知识库；机制缺口见 [精英干员组后端缺口](精英干员组后端缺口.md) 与 [docs/TODO/…](../TODO/精英干员组后端缺口.md)。

## 责任边界

- 玩家知识层：缺口、目标、完成度、展示顺序。
- `核心/重要/次级/挂件` ≠ solver hard admission。
- 直接访问 `advice` 不先跑 solver；有求解上下文时才可消费布局/效率。
- `data/` 不裁决 Markdown 语义。

## Agent 路由

| 意图 | 首读 |
|---|---|
| 玩家语义 / 名单 / 口径 / 组合 | 本文 → 对应 canonical |
| 实现玩家层 | [玩家报告实现计划](玩家报告实现计划.md) + feature skill |
| 旧 JSON / 旧 now·conditional | 归档 v2 合同；结论不得覆盖本目录 |
| 公孙验收 | `gongsun-training-review` |

## 当前实现状态

**2026-08-05：** 阶段 A/B/C/E 已合入：独立 knowledge schema、完整新手/17 组/散件知识、`player` schema v2 完成度与优先级输出，以及 `plan.compute.training_advice` 前端接口。T9 由排班结果中各班各站的 `skill-efficiency ÷ 当班人数` 简单平均计算；动态金属工艺/莱茵科技成员由技能名自动枚举；同站条件只作说明，源石线和泡泡双人门闩仍是行动门闩；“部分可用/30% 合格形态”已废弃。新前端直接使用 `plan.compute.training_advice`，旧 v2 `training_recommendations.json`/`evaluate`/`rag` 仅保留为独立诊断工具，不作为前端数据源；五章自然语言报告和 `--answer` 留给未来 AI。

## 新干员事实来源

2026-07-20 解包与 PRTS 交叉核对摘要（机械师、谬因、可露希尔、凯尔希·思衡托）仍有效；细节见会话归档与技能/实例数据，不在此重复展开。
