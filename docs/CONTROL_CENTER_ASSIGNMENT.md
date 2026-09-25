# 控制中枢编制与效果账本

> 文档角色：canonical
> 生命周期状态：current
> 领域键：facility.control-assignment
> 当前真源：self
> 摘要：说明控制中枢的 required anchor、顺序选位、EffectLedger 和全局注入责任边界。
> 代码范围：`crates/infra-core/src/control/{input,interpreter,ledger,mod}.rs`; `search/control.rs`; `pool/control.rs`; `layout/assign/{control_fill,pipeline}.rs`; `global_resource/inject.rs`
> 代码锚点：`crates/infra-core/src/control/interpreter.rs::{solve_control,solve_control_with_trace}`; `crates/infra-core/src/control/ledger.rs::ControlEffectLedger`; `crates/infra-core/src/control/mod.rs::apply_control_to_layout_with_trace`; `crates/infra-core/src/search/control.rs::search_control_combos`; `crates/infra-core/src/layout/assign/control_fill.rs::assign_control`; `crates/infra-core/src/layout/assign/pipeline.rs::run_shift_pipeline`
> 关联链路：`docs/模块/中枢与跨设施状态.md`; `docs/调用链/跨设施状态刷新.md`; `docs/模块/候选搜索与Bake.md`
> 核验基线：`947f1a2`

## 编制硬约束

中枢容量由 blueprint 决定，当前标准布局为 5 人。可用角色足够时先满足 Plan 的 required anchor，再补齐合法席位；无中枢技能的 filler 仍可能提供 occupancy/mood 或满足跨设施条件。后续 filler 不能替换已认领的 required anchor。缺席、重复 operator、同房禁配和无法满足 required group 返回错误/无解。

`search/control.rs` 的 `ControlFillPolicy` 是顺序 policy，不是全基建匿名总分。排序依次消费 Plan anchor、贸易/制造全局区域、训练/办公室/会客室、心情去重和普通补位；不回溯替换前序结果。

## Ledger 事实

`ControlEffectKey {domain, unit}` 是结构化 key。`record_max` 记录候选来源，`freeze` 对同 key 取最大值，并标记 `Winner`、`TiedWinner` 或 `Suppressed`。注入、trace、candidate dependency 和投影视图都应读取冻结 ledger；不得按 operator 名、buff 前缀或数值单位猜同类效果。冲突来源仍是物理占位冲突，即使低值 atom 已 suppressed。

`solve_control_with_trace` 返回 `ControlCenterResult` 和 `ControlSettlementTrace`；`apply_control_to_layout_with_trace` 把 global pool、`GlobalInjectManifest`、办公室和会客室速度写回 `LayoutContext`。下游 trade/manufacture 只消费 resolved facts。

## 动态 producer

中枢 producer 的 admission、target、relation 由 `data/producer_rules.json`/Plan 解析；source 只有实际 assignment 在场时才进入 manifest。producer 变化应触发 resolve 和下一阶段搜索，不能靠标签存在或 export 补偿。

## 核验

检查 control trace、ledger records、global inject explanation、最终 assignment invariant。固定候选用 `layout eval --explain-json`；整班用 `plan --assert-invariants` 和 243/high-owned fixture。中枢技能原文与同种效果规则回知识库引用，本文只记录实现责任和状态流。
