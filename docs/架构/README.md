# 架构入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/架构/BASE_ASSIGNMENT.md；docs/架构/ORCHESTRATION_LAYER.md；docs/架构/SYSTEM_CHAINS.md
> 摘要：按 Plan、状态分配、跨设施编排和体系链路解释求解器架构

## 阅读地图

| 问题 | 入口 |
|---|---|
| `build_plan -> execute_plan -> fill -> resolve` 如何衔接 | [BaseAssignment 与分配主路径](BASE_ASSIGNMENT.md) |
| Rule 如何编译成 Plan，谁负责执行 | [编排层](ORCHESTRATION_LAYER.md) |
| 体系、anchor、producer/consumer 如何进入链路 | [体系链路索引](SYSTEM_CHAINS.md) |
| 为什么用有限组合、统一量纲和阶段责任 | [项目逻辑内核](项目逻辑内核.md) |
| 具体代码 owner 和命令 | [项目地图](../PROJECT_MAP.md) |

开放求解架构与责任边界工作从 [核心求解工作流](../TODO/核心求解.md) 进入。

## 本领域全部文档

<!-- BEGIN GENERATED DOMAIN DOCUMENTS -->
| 文档 | 角色 | 状态 | 摘要 |
|---|---|---|---|
| [全基建单班进驻编制](BASE_ASSIGNMENT.md) | `current-reference` | `current` | 说明单班 BaseAssignment 的 Plan、落位、互斥和 resolve 责任 |
| [编排层](ORCHESTRATION_LAYER.md) | `current-reference` | `current` | 说明声明式规则如何编译为 AssignmentPlan 并交给 executor |
| [基建体系链入口](SYSTEM_CHAINS.md) | `current-reference` | `current` | 导航已注册体系 capability、Plan 和运行时 owner，不复制机制正文 |
| [项目逻辑内核](项目逻辑内核.md) | `current-reference` | `current` | 解释有限组合、统一量纲与阶段责任为何构成本项目内核，给后续Agent提供维护方向 |
<!-- END GENERATED DOMAIN DOCUMENTS -->
