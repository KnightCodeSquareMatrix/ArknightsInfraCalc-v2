# 当前实现状态入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md；docs/PROJECT_MAP.md；docs/QUALITY_AND_AUDIT.md
> 摘要：为高频维护任务提供当前实现、限制和代码入口的短路由

本文是维护入口，不是新的业务真源。它只回答“当前代码大致做到哪里、下一步应查哪里”；领域规则仍由 [文档入口](INDEX.md) 路由到唯一 canonical，历史过程仍由案件卡、ADR 和归档材料承载。

## 高频维护域

| 任务域 | 当前实现入口 | 主要限制 | 首读路径 |
|---|---|---|---|
| 贸易候选与单位产出 | `layout/assign/trade_fill.rs`、`search/role_pick.rs`、`search/trade.rs`、`trade/{shortcut,solver}.rs` | 候选准入、L2/L3 结算和跨房状态不能从一次 top hit 推断；具体保证等级按案件确认 | [贸易内部地图](INTERNAL/README.md) → [贸易解释器](INTERNAL/TRADE_INTERPRETER.md) → [shortcut 匹配](INTERNAL/SHORTCUT_MATCHING.md) |
| 排班与菲亚梅塔 | `schedule/team_rotation.rs`、`schedule/shift_bind.rs`、`export/maa.rs` | 当前输出表达已决定的动作和绑定；是否保证执行侧回宿舍，取决于现有输出契约 | [排班模式](排班模式.md) → [菲亚梅塔规则](菲亚梅塔使用规则.md) → [当前实现](SCHEDULE_ROTATION.md) |
| 中枢、全局资源与 layout | `control/`、`layout/assign/pipeline.rs`、`layout/resolve.rs`、`layout/orchestrate/` | Plan admission、Global atom 和 per-room 扣回必须区分领域裁决与当前代码；已知描述冲突见 [跨设施内部地图](INTERNAL/CROSS_FACILITY.md) | [控制中枢](CONTROL_CENTER_ASSIGNMENT.md) → [项目地图](PROJECT_MAP.md) |
| Bake、候选和保证 | `bake.rs`、`pool/`、`search/`、`commands/{bake,profile}.rs` | Bake 是加速载体；schema、输入、候选结构或内容不兼容时必须回 live；不得把 policy 或 approximation 宣称为全局最优 | [质量总则](QUALITY_AND_AUDIT.md) → [性能工程](PERFORMANCE_ENGINEERING.md) |
| Worker 与前端调用 | `crates/infra-cli/src/commands/{plan_compute,serve}.rs`、`contracts/plan-compute-v*.schema.json` | `plan`、`plan.compute` 和 legacy 路径的部署状态要以目标环境验收为准；接口语义不能从 TODO 推断 | [前端接口](FRONTEND_CLI.md) → [serve 指南](FRONTEND_SERVE_GUIDE.md) → [项目地图](PROJECT_MAP.md) |

## 当前案件

以下链接只表示相关工作入口，不表示案件已经完成或代码已经满足目标：

- [08-212 贸易站龙巫裁缝候选问题](ARCHIVE/done/08-212贸易站龙巫裁缝候选问题记录.md)：`completed`，已完成 β >> α > 白板准入修复与最终状态门禁；批量验证保留一个无关的 35-343 中枢求解失败。
- [排班表求解器严重问题](TODO/排班表求解器严重bug分析-20260827.md)：`blocked`，涉及中枢资源和多个可能的领域政策分歧。
- [发电效率共享基础重复计算](TODO/发电效率共享基础重复计算分析-20260827.md)：实现状态与最终 assignment / 批量验证需要分开确认。
- [菲亚梅塔宿舍回岗可用性](TODO/菲亚梅塔宿舍回岗可用性缺口.md)：`proposal`，首先需要裁决输出契约是否扩展，不默认视为 bug。

## 使用边界

- 先由根 `AGENTS.md` 选择任务 Skill，再按本文定位代码和当前参考。
- 本文不复制公式、候选名单、完整 schema、fixture 真值或历史审计结论。
- 理论与实现不一致时，在对应案件中并列记录；只有用户裁决或 canonical 变更授权才能更新领域规范。
- 代码 owner 细节以 [项目地图](PROJECT_MAP.md) 和实际源码为准；本文过期时修正路由，不在此重建业务模型。
