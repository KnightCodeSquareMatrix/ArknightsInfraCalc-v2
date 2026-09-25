# 制造站求解状态

> 文档角色：canonical
> 生命周期状态：current
> 摘要：裁决制造站输入、阶段解释、产线聚合、仓储和班次心情结算边界。
> 领域键：facility.manufacture
> 当前真源：self
> 代码范围：`crates/infra-core/src/manufacture/{input,interpreter,solver}.rs`; `crates/infra-core/src/pool/manufacture.rs`; `crates/infra-core/src/search/manufacture.rs`; `crates/infra-core/src/layout/assign/manufacture_fill.rs`; `data/prts_manufacturing_skills.json`
> 代码锚点：`crates/infra-core/src/manufacture/interpreter.rs::apply_manu_phases`; `crates/infra-core/src/manufacture/solver.rs::{solve_manufacture,solve_manufacture_with_shift,evaluate_manufacture_lines}`; `crates/infra-core/src/search/manufacture.rs::search_manufacture_triples`; `crates/infra-core/src/pool/manufacture.rs::build_manufacture_pool`; `crates/infra-core/src/layout/assign/manufacture_fill.rs::assign_manu_room_with_anchors`
> 关联链路：`docs/模块/制造求解.md`; `docs/调用链/技能到房间结算.md`; `docs/调用链/跨设施状态刷新.md`
> 核验基线：`947f1a2`

## 责任边界

制造 solver 接收已经确定的房间、配方、实际三人组、LayoutContext 和可选班次时长，输出生产效率、单位日产量、仓库上限、心情消耗和分解。它不决定 control/trade 的候选，也没有贸易站的订单分布、gold flow 或 shortcut。

## 结算顺序

```text
ManuRoomInput
  -> ManuContext::from_room
  -> collect_manu_atoms / apply_manu_phases
  -> RecipeEff + RecipeLimit + state/global inject
  -> solve_manufacture[_with_shift]
  -> ManuResult / ManuProdBreakdown / ManuStorageBreakdown
```

`apply_manu_phases` 先收集并按 `Phase::sort_key + phase_order` 执行 atom，再处理制造专属 tagged inject、配方条件、PeerAbsorb、仓储与心情。`solve_manufacture_with_shift` 额外在班次/心情 gap 上求值；静态探索传 `None` 或调用 `solve_manufacture`。多线由 `evaluate_manufacture_lines` 汇总，不能由 CLI 手拼。

## 搜索与排班

`build_manufacture_pool` 将 OperBox 的 facility binding、tier 和 `SkillTable` 映射为 `ManuPoolEntry`；缺失实例/buff 记录 skip，不是白板高效技能。`search_manufacture_triples` 先按 `full_pool` 过滤，再对单配方做 `C(n,3)` 组合；`ManuSearchRecipeMode::Lines` 分别求赤金和作战记录线并按线数合并。排班填房使用 `manu_options`、Plan anchor、`used` 和最新 resolve 快照。

## 失败与回退

配方/等级/人数非法直接返回错误；单配方无解返回无 hit/错误，由上层决定是否换配方。心情不足返回带 gap 的结果，排班 policy 决定休息或换班；制造 solver 不修改宿舍。全局资源未刷新时检查 `resolve_base` 与 cross-facility 链，不在制造结果出口补百分比。Bake 不兼容时回退 live search。

核验入口：`layout eval --explain-json`、`infra-cli bench`、制造搜索报告和 `plan --assert-invariants`。机制数字来自知识库桥接；运行时 JSON 只作为输入载体。
