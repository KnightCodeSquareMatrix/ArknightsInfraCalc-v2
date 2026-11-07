# 排班入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/排班/排班模式.md；docs/排班/定时换班.md；docs/排班/自动轮换.md
> 摘要：按 profile、班次构造、菲亚梅塔和源石优先规则定位当前排班语义

## 阅读地图

| 问题 | 入口 |
|---|---|
| 当前有哪些 profile、哪些已删除 | [排班模式](排班模式.md) |
| 固定时刻定时换班的完整语义 | [定时换班](定时换班.md) |
| 自动轮换与定时换轮换的边界 | [自动轮换](自动轮换.md) |
| 菲亚梅塔动作和候选分支 | [菲亚梅塔使用规则](菲亚梅塔使用规则.md)、[候选分支适配层](菲亚梅塔候选分支适配层设计.md) |
| 泡泡/火神首个源石房预占 | [泡泡火神源石优先](泡泡火神源石优先.md) |
| 短入口和完整调用链 | [排班轮换短入口](SCHEDULE_ROTATION.md) |

开放排班工作从 [排班轮换工作流](../TODO/排班轮换.md) 进入；已完成调查只用于[历史追溯](../ARCHIVE/README.md)，不能覆盖 canonical。

## 本领域全部文档

<!-- BEGIN GENERATED DOMAIN DOCUMENTS -->
| 文档 | 角色 | 状态 | 摘要 |
|---|---|---|---|
| [菲亚梅塔换心情规则（导航）](Fiammetta.md) | `current-reference` | `current` | 说明菲亚梅塔动作由排班决定并由 MAA 导出的责任边界 |
| [排班轮换](SCHEDULE_ROTATION.md) | `current-reference` | `current` | 提供排班轮换的短入口、profile 表和完整调用链链接 |
| [定时换班](定时换班.md) | `canonical` | `current` | 定义固定时刻定时轮换的求解器行为和输出边界 |
| [排班模式](排班模式.md) | `canonical` | `current` | 定义当前支持的排班 profile、入口参数和模式边界 |
| [泡泡火神源石优先](泡泡火神源石优先.md) | `canonical` | `current` | 定义普通二班/三班定时轮换中泡泡与火神强制占用首个合法源石制造房的语义、保护边界和失败输出 |
| [自动轮换](自动轮换.md) | `canonical` | `current` | 定义自动轮换独立排班模式的已裁决语义、与定时轮换的边界、禁用项和未裁决项 |
| [菲亚梅塔使用规则](菲亚梅塔使用规则.md) | `canonical` | `current` | 定义菲亚梅塔动作数量、目标去重和输出责任 |
| [菲亚梅塔候选分支适配层设计](菲亚梅塔候选分支适配层设计.md) | `canonical` | `current` | 裁决 ABC 三班菲亚梅塔候选分支适配层的职责边界、候选差异机制、动作语义、贸易组合竞争规则与产物价值比较 |
<!-- END GENERATED DOMAIN DOCUMENTS -->
