# 指南入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/INDEX.md；docs/架构/ORCHESTRATION_LAYER.md；docs/质量与维护/MAINTENANCE_MODE.md
> 摘要：为新读者和维护者提供项目范围、架构、术语和任务入口的阅读顺序

## 推荐阅读顺序

| 目标 | 先读 |
|---|---|
| 五分钟了解项目边界 | [项目总览](OVERVIEW.md) |
| 走一遍主调用链 | [架构导览](ARCHITECTURE_TOUR.md) |
| 第一次完整运行 `plan` | [243 全精二完整示例](../EXAMPLES/243_FULL_E2.md) |
| 查当前能力和已知限制 | [当前状态](CURRENT_STATUS.md) |
| 查项目术语 | [术语表](GLOSSARY.md) |
| 用 Agent 检索项目知识 | [Agent 知识库](Agent知识库.md) |
| 判断旧实现能否删除 | [检查与删减导航](检查与删减导航.md) |

领域机制沿 [机制入口](../机制/README.md) 阅读，代码 owner 查 [项目地图](../PROJECT_MAP.md)，活动工作从 [TODO 总板](../TODO/README.md) 进入。

## 本领域全部文档

<!-- BEGIN GENERATED DOMAIN DOCUMENTS -->
| 文档 | 角色 | 状态 | 摘要 |
|---|---|---|---|
| [架构导览：从 `plan` 到 MAA 排班](ARCHITECTURE_TOUR.md) | `current-reference` | `current` | 解释当前核心调用链和模块边界 |
| [Agent 知识库](Agent知识库.md) | `current-reference` | `current` | 定义本地 Agent 检索入口、来源身份、历史过滤、回取与更新方式；领域事实仍由原 owner 裁决 |
| [当前实现状态入口](CURRENT_STATUS.md) | `current-reference` | `current` | 为高频维护任务提供当前实现、限制和代码入口的短路由 |
| [术语表](GLOSSARY.md) | `current-reference` | `current` | 以短定义把项目术语路由到各 canonical 或 current owner |
| [给策略作者的项目运行说明](GONGSUN_RUNTIME_OVERVIEW.md) | `current-reference` | `current` | 面向基建策略作者解释当前求解流程和策略输入边界 |
| [项目总览：从技能语义到 MAA 排班](OVERVIEW.md) | `current-reference` | `current` | 提供项目当前范围和能力总览 |
| [检查与删减导航](检查与删减导航.md) | `current-reference` | `current` | 按检查责任导航删除依据、保留条件、失效前提和真实 owner，不作为第二份业务裁决 |
<!-- END GENERATED DOMAIN DOCUMENTS -->
