# 发电与无人机结算

> 文档角色：canonical
> 生命周期状态：current
> 摘要：说明发电效率、无人机库存和排班/导出动作之间的结算边界。
> 领域键：production.drones
> 当前真源：self
> 代码范围：`crates/infra-core/src/power/{input,interpreter,solver}.rs`; `crates/infra-core/src/layout/resolve.rs`; `crates/infra-core/src/layout/assignment.rs`; `crates/infra-core/src/schedule/*`; `crates/infra-core/src/export/maa.rs`
> 代码锚点：`crates/infra-core/src/power/solver.rs::{aggregate_power_efficiency,solve_power,apply_power_to_layout}`; `crates/infra-core/src/power/interpreter.rs::apply_power_phases`; `crates/infra-core/src/layout/resolve.rs::build_power_rooms`; `crates/infra-core/src/schedule/team_rotation.rs`; `crates/infra-core/src/export/maa.rs`
> 关联链路：`docs/模块/心情与辅助设施.md`; `docs/调用链/跨设施状态刷新.md`
> 核验基线：`947f1a2`

## 两条边界

发电站效率产生无人机充能速度；无人机库存再由 rotation/export 的 `DroneAction` 投向制造或贸易。`drone_cap` 是布局输入中的库存上限，不是当前库存，也不是固定效率。本文不定义换班目标或 MAA 文本格式。

## 发电结算

`PowerRoomInput` 经过 `apply_power_phases`，再由 `solve_power` 输出单站 `PowerResult`。`aggregate_power_efficiency` 只汇总岗位增量；`apply_power_to_layout` 把结果写进 `LayoutContext`，共享基础值在布局总量中只计一次。心情为零的进驻者仍占位，但是否提供技能/作业平台增量由 power interpreter 和数据条件决定。

```text
assignment power rooms
  -> resolve_base / build_power_rooms
  -> power interpreter + solve_power
  -> LayoutContext power result
  -> schedule DroneAction / export
```

发电站数量、平台数量和 control/global inject 通过 `LayoutContext` 传给贸易/制造 atom；不要在出口层重新按房间数加成。

## 诊断和核验

无人机异常先对比 assignment、power breakdown、有效心情、`LayoutContext` 的 power counts，再看 rotation 的 DroneAction 和 export。固定 assignment 用 `layout eval --explain-json`；完整班次用 `plan --assert-invariants` 和代表性 243/high-owned fixture。当前实现不等于已覆盖所有游戏内无人机策略，未建模机制应在知识库桥接中标注。
