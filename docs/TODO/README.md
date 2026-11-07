# TODO 任务总板

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md；docs/INDEX.md
> 摘要：按五个工作流汇总开放活动项；详情页拥有目标、证据和关闭条件

本目录只容纳尚未吸收进 current owner 的 `active-change`。它是工作登记簿，不是默认队列；只有用户当前指令明确选择某项工作时才获得实施授权。`docs/TODO/` 顶层固定为总板、五个工作流和 `详情/`；详情及其附件放入 `详情/<工作流>/`，不再散落在 TODO 根目录。

## 如何选择工作流

| 入口 | 典型问题 |
|---|---|
| [核心求解](核心求解.md) | 候选没进、错误人选、Plan/admission、跨设施状态、机制覆盖 |
| [排班轮换](排班轮换.md) | 班次非法、profile、菲亚/回收站、休息、回岗、Team/Shift |
| [接口产品](接口产品.md) | Worker/CLI wire、前端版本、训练室输出、网站产品 |
| [性能质量](性能质量.md) | 正确性归因、搜索热路径、Bake、cache、批量验证 |
| [文档协作](文档协作.md) | Agent 规则、文档 owner、导航、生成索引和门禁 |

## 工作流概览

<!-- BEGIN GENERATED WORKSTREAMS -->
| 工作流 | 活动项 | 状态分布 | 入口 |
|---|---:|---|---|
| 核心求解 | 11 | `proposal` 4、`ready-on-request` 7 | [核心求解](核心求解.md) |
| 排班轮换 | 11 | `blocked` 2、`proposal` 6、`ready-on-request` 3 | [排班轮换](排班轮换.md) |
| 接口产品 | 6 | `blocked` 1、`proposal` 3、`ready-on-request` 2 | [接口产品](接口产品.md) |
| 性能质量 | 7 | `proposal` 3、`ready-on-request` 4 | [性能质量](性能质量.md) |
| 文档协作 | 2 | `blocked` 1、`ready-on-request` 1 | [文档协作](文档协作.md) |
<!-- END GENERATED WORKSTREAMS -->

## 状态与使用边界

| 状态 | 含义 |
|---|---|
| `proposal` | 方向或语义仍需裁决，不能直接实施 |
| `ready-on-request` | 目标和边界已足够清楚，可由用户当前指令恢复 |
| `in-progress` | 当前工作树存在 writer；默认分支不应保留该状态 |
| `blocked` | 有明确外部依赖或语义冲突；详情页必须写阻塞条件 |

关闭活动项时，先把确认事实吸收到一个 current owner，再把仍开放内容拆到新详情，最后把原文和证据移入 [ARCHIVE](../ARCHIVE/README.md)。不要只把状态改成完成，也不要让 tracker 复制详情正文。归档与索引规则见 [文档生命周期](../文档生命周期.md)。

新建任务使用可搜索的中文标题，并在详情页声明 current owner、目标、非目标、成功标准和验证入口。文件路径与状态只提供导航；用户裁决和领域 canonical 仍决定实际语义。
