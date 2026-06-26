# 文档入口

本文是人类和 AI 进入项目文档的总入口。不要从 `docs/` 全量通读；按任务路由到对应文档。

## 首读

| 文档 | 用途 |
|------|------|
| [../AGENTS.md](../AGENTS.md) | Agent 操作规则、当前主线、验证命令 |
| [PROJECT_MAP.md](PROJECT_MAP.md) | 项目结构、模块索引、数据真源 |
| [GONGSUN_RUNTIME_OVERVIEW.md](GONGSUN_RUNTIME_OVERVIEW.md) | 给懂基建但不懂代码的公孙长乐解释项目运行流程 |
| [TODO/README.md](TODO/README.md) | 准备实现的功能与当前工作队列 |
| [ARCHIVE/README.md](ARCHIVE/README.md) | 已完成、废弃或历史设计材料 |

## 按任务阅读

| 任务 | 读这些 |
|------|--------|
| 改评分 / 排序 / 分量口径 | [SCORING_REFACTOR_PLAN.md](SCORING_REFACTOR_PLAN.md)、[SCORING_MODEL.md](SCORING_MODEL.md) |
| 改编排 / 体系 / meta 组合 | [ADR/0001-layout-assignment-decomposition.md](ADR/0001-layout-assignment-decomposition.md)、[ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)、[BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md) |
| 加体系 anchor / 代码化体系层 / 降级 | [TODO/CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md](TODO/CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md)、[TODO/SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md](TODO/SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md) |
| 给基建策略作者解释程序运行过程 | [GONGSUN_RUNTIME_OVERVIEW.md](GONGSUN_RUNTIME_OVERVIEW.md) |
| 改排班轮换 / MAA 导出 | [SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md)、[INFRA_CLI.md](INFRA_CLI.md) |
| 改 CLI / 前端集成 | [INFRA_CLI.md](INFRA_CLI.md)、[FRONTEND_CLI.md](FRONTEND_CLI.md) |
| 改贸易 L1/L2/L3 | [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md)、[INTERNAL/TRADE_INTERPRETER.md](INTERNAL/TRADE_INTERPRETER.md)、[INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md) |
| 改制造站 | [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md) |
| 改跨设施 global atom | [INTERNAL/CROSS_FACILITY.md](INTERNAL/CROSS_FACILITY.md)、[EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) |
| 查已建模 / 待建模干员 | [MODELLED_OPERATORS.md](MODELLED_OPERATORS.md)、[需要完成的干员建模.md](需要完成的干员建模.md) |

## 文档分层

| 层级 | 位置 | 规则 |
|------|------|------|
| 入口 | `README.md`、`AGENTS.md`、本文 | 只放路由、当前主线、常用命令 |
| 架构参考 | `PROJECT_MAP.md`、`*_STATUS.md`、`*_MODEL.md` | 记录当前事实，不写长篇历史推演 |
| 实施计划 | `docs/TODO/` | 只放准备实现或正在实现的事项 |
| 架构决策 | `docs/ADR/` | 已接受的结构性决策；记录为什么这样拆，不放执行清单 |
| 细节地图 | `docs/INTERNAL/` | 给千行级文件做函数段导航 |
| 理论参考 | `docs/公孙长乐的体系分析文档/`、`SYSTEM_CHAINS.md` | 记录体系理论和锚点，不作为代码入口 |
| 归档 | `docs/ARCHIVE/`、`plans/` | 已完成、废弃、不再首读的材料 |

## 维护规则

1. 新功能准备实现时，在 `docs/TODO/` 新建一个 Markdown，写清范围、入口文件、验收命令。
2. 功能完成后，把对应 TODO 移到 `docs/ARCHIVE/`，并在原计划或主文档里更新状态。
3. `PROJECT_MAP.md` 只记录当前架构事实；不要把历史讨论继续塞进去。
4. `plans/` 默认视为历史设计记录；只有当前文档明确引用时才读。
