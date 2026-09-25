# cross_facility 内部地图

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/模块/中枢与跨设施状态.md
> 代码范围：`crates/infra-core/src/cross_facility/{collector,interpreter,mod}.rs`; `crates/infra-core/src/global_resource/{pool,inject,key,registry}.rs`; `crates/infra-core/src/layout/resolve.rs`
> 代码锚点：`crates/infra-core/src/cross_facility/collector.rs::{GlobalAtomEntry,collect_global_atoms}`; `crates/infra-core/src/cross_facility/interpreter.rs::orchestrate_global_atoms`; `crates/infra-core/src/cross_facility/interpreter.rs::{condition_met,apply_state_write}`; `crates/infra-core/src/global_resource/pool.rs::GlobalResourcePool`; `crates/infra-core/src/global_resource/inject.rs::GlobalInjectManifest`
> 关联链路：`docs/模块/中枢与跨设施状态.md`; `docs/调用链/跨设施状态刷新.md`
> 核验基线：`947f1a2`

collector 遍历当前 BaseAssignment，按设施、tier、binding 找到 buff ids，只收集 `atom.scope == Global`，并按 phase 排序。宿舍/办公室低练度 global atom 默认跳过，`SilentEcho` 是显式例外。interpreter 当前只实际执行 `StateWrite`（StateProduce/StateConvert）；其他 phase 留给 control 或房间域。

`resolve_base` 将结果合并到 `LayoutContext.global`，再由 global resource/inject 和各域输入消费。Global atom 不应在 per-room interpreter 再算；否则会重复贡献。未知 key、不可用 selector 或未满足 admission 应保持 delegated/零贡献，不注入假 buff。
