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
  -> apply_manu_phases（借用预编译 atom，按阶段稳定归并）
  -> RecipeEff + RecipeLimit + state/global inject
  -> solve_manufacture[_with_shift]
  -> ManuResult / ManuProdBreakdown / ManuStorageBreakdown
```

`apply_manu_phases` 借用各来源的预编译 atom，按阶段、owner 顺序与原子序号稳定归并执行；处理制造专属 tagged inject、配方条件、PeerAbsorb、仓储与心情。`solve_manufacture_with_shift` 额外在班次/心情 gap 上求值；静态探索传 `None` 或调用 `solve_manufacture`。多线由 `evaluate_manufacture_lines` 汇总，不能由 CLI 手拼。

制造暖机按 `Action::AddEffRamp` 分派：有班长时取 `round(h)` 并限制在 1～24 小时，计算逐时加成的算术平均；无班长保持原 20 小时。平均贡献先进入原解释器的技能面，再被后续原子消费。这是用户裁决的整班等效效率口径（approximation），不声明逐时非线性积分；例如他人技能求和后的取整仍在平均之后进行。铅踝的各心情区间及结束仓库结算共用同一个整班暖机窗口，不用区间长度重启。非有限班长仍报错；有限零/负班长不做心情分段，暖机窗口为 1 小时。轻量排序与完整输出共享同一窗口规则。来源：固定知识库制造站机制「时间爬升」及技能制造站中的五条暖机原文；运行时映射为 `skill_table.json` 的 `add_eff_ramp` → 制造解释器。

带班长且查询池含暖机 atom 时，Bake 整个查询回 live，并记 `ramp`；无班长的固定 20 小时行保留生成与全量对账。该门禁不删除候选。最终统一对拍与暖机差异审计尚未完成。

## 搜索与排班

`build_manufacture_pool` 将 OperBox 的 facility binding、tier 和 `SkillTable` 映射为 `ManuPoolEntry`；缺失实例/buff 记录 skip，不是白板高效技能。`search_manufacture_triples` 先按 `full_pool` 过滤，再对单配方做 `C(n,3)` 组合；`ManuSearchRecipeMode::Lines` 分别求赤金和作战记录线并按线数合并。排班填房使用 `manu_options`、Plan anchor、`used` 和最新 resolve 快照。

## 失败与回退

配方/等级/人数非法直接返回错误；单配方无解返回无 hit/错误，由上层决定是否换配方。心情不足返回带 gap 的结果，排班 policy 决定休息或换班；制造 solver 不修改宿舍。全局资源未刷新时检查 `resolve_base` 与 cross-facility 链，不在制造结果出口补百分比。Bake 不兼容时回退 live search。

核验入口：`layout eval --explain-json`、`infra-cli bench`、制造搜索报告和 `plan --assert-invariants`。机制数字来自知识库桥接；运行时 JSON 只作为输入载体。
