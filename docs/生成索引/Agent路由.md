# Agent 路由（生成）

> 文档角色：generated-reference
> 生命周期状态：generated
> 当前真源：docs/agent-index/README.md
> 生成器：scripts/codex/generate_agent_index.py
> 摘要：由稳定 route 注册表生成的症状、入口、调用链、模块、机制和验证导航

<!-- GENERATED: do not edit -->
## `CLI.PLAN`：完整 plan 请求

**症状**：plan 失败、profile 不对、MAA 与文本不一致、完整方案

**入口**：`crates/infra-cli/src/main.rs::run`；`crates/infra-cli/src/commands/plan_compute.rs::compute_plan`

**完整调用链**：[docs/调用链/plan请求.md](../调用链/plan请求.md)

**模块 owner**：[docs/接口/CLI与Worker.md](../接口/CLI与Worker.md)；[docs/模块/排班与轮换.md](../模块/排班与轮换.md)

**机制引用**：[docs/知识库引用.md#solver-文档的引用格式](../知识库引用.md#solver-文档的引用格式)

**验证**：`infra-cli plan --operbox <path> --profile-out out/<task>-profile.json --maa-out out/<task>-maa.json`

## `LAYOUT.SHIFT`：单班布局与设施填充

**症状**：干员被抢、体系未激活、房间没填满、候选池异常

**入口**：`crates/infra-core/src/layout/assign.rs::assign_shift_with_plan_skip`；`crates/infra-core/src/layout/orchestrate/select.rs::build_plan`

**完整调用链**：[docs/调用链/跨设施状态刷新.md](../调用链/跨设施状态刷新.md)

**模块 owner**：[docs/模块/候选搜索与Bake.md](../模块/候选搜索与Bake.md)；[docs/模块/中枢与跨设施状态.md](../模块/中枢与跨设施状态.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**验证**：`infra-cli layout test --layout <path> --operbox <path>`

## `SOLVER.TRADE_ROOM`：贸易站单房结算

**症状**：贸易效率错误、订单类型错误、shortcut 未命中、单位产出异常

**入口**：`crates/infra-core/src/trade/solver.rs::solve_trade_with_shift`；`crates/infra-core/src/trade/interpreter.rs`

**完整调用链**：[docs/调用链/技能到房间结算.md](../调用链/技能到房间结算.md)

**模块 owner**：[docs/模块/贸易求解.md](../模块/贸易求解.md)；[docs/模块/输入与技能映射.md](../模块/输入与技能映射.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**验证**：`infra-cli verify --all`；`infra-cli trade yield <fixture>`

## `ROTATION.TIMED`：定时轮换

**症状**：班次互斥错误、同上同下错误、profile 不识别、休息状态错误

**入口**：`crates/infra-core/src/schedule/team_rotation.rs::schedule_timed_rotation`

**完整调用链**：[docs/调用链/定时轮换.md](../调用链/定时轮换.md)

**模块 owner**：[docs/模块/排班与轮换.md](../模块/排班与轮换.md)；[docs/模块/导出与岗位状态.md](../模块/导出与岗位状态.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**验证**：`infra-cli layout team-rotation --layout <path> --operbox <path> --json`

## `EXPORT.MAA`：MAA 导出

**症状**：MAA 字段错误、导出房间与 assignment 不同、轮换数量错误

**入口**：`crates/infra-core/src/export/maa.rs::build_from_team_rotation`

**完整调用链**：[docs/调用链/MAA导出.md](../调用链/MAA导出.md)

**模块 owner**：[docs/模块/导出与岗位状态.md](../模块/导出与岗位状态.md)；[docs/接口/输入输出与兼容.md](../接口/输入输出与兼容.md)

**机制引用**：[docs/知识库引用.md#唯一边界](../知识库引用.md#唯一边界)

**验证**：`infra-cli plan --operbox <path> --maa-out out/<task>-maa.json`

## `DIAGNOSTICS.DEBUG`：debug bundle 重放

**症状**：线上反馈复现、求解错误、某房间异常、某干员未进池

**入口**：`crates/infra-cli/src/commands/debug.rs::debug_cmd`

**完整调用链**：[docs/调用链/debug与验证.md](../调用链/debug与验证.md)

**模块 owner**：[docs/接口/错误与诊断.md](../接口/错误与诊断.md)；[docs/模块/候选搜索与Bake.md](../模块/候选搜索与Bake.md)

**机制引用**：[docs/知识库引用.md#不可混淆的四种内容](../知识库引用.md#不可混淆的四种内容)

**验证**：`infra-cli debug rerun --bundle <debug-bundle.json>`
