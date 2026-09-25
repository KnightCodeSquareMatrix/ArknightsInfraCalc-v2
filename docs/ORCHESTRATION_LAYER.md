# 编排层

> 文档角色：module-reference
> 生命周期状态：current
> 当前真源：docs/BASE_ASSIGNMENT.md
> 摘要：说明声明式规则如何编译为 AssignmentPlan 并交给 executor
> 代码范围：crates/infra-core/src/layout/orchestrate/、layout/assign/、data/orchestration_rules.json
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：build_plan、execute_plan、SystemAnchor、AssignmentPlan
> 关联链路：docs/BASE_ASSIGNMENT.md、docs/SYSTEM_CHAINS.md、docs/模块/排班与轮换.md
> 核验基线：Plan owner、resolved dependency、execute 后 BaseAssignment

编排层把声明式 system/alternative 编译成有限 Plan，再由 executor 执行；负责 required admission、scope、anchor、dependency 和 Plan owner。设施 solver 负责房间效率，schedule 负责跨班轮换。

    rules / knowledge refs
      → build_plan（有限 alternatives）
      → execute_plan（anchor/dependency）
      → assign_shift / fill
      → AssignmentPlan + BaseAssignment

build_plan 解析 PickOne、OptionalFixed、required count、scope 和 alternatives，不写最终房间。execute_plan 消费 resolved facts，提交 cross-station/same-station claims 和 dependency。shift_bind 只能消费 Plan 实际 bind；tag、priority、shortcut 不能替代 required admission。

当前策略是跨站体系先于同站候选、producer/consumer 以实际 presence 形成 dependency、制造逐房重算 resolve_base、fallback 只限声明式 role/alternative。旧路线图、删除 helper 和历史审计仅在 ARCHIVE 追溯。

## 变更时的调用链定位

- 组合未进入岗位：先查 build_plan 是否有 required admission/alternative，再查 execute_plan 是否提交 anchor；不要先调 facility solver。
- 组合进入但收益为零：查 runtime capability、scope 和实际同房/同班 presence，再查 resolve_base；不要按名字增加 shortcut。
- 跨房 consumer 缺 producer：查 resolved_producer_dependencies 是否产生 exact_presence/requires_presence；没有 dependency 时 schedule 不应生成 shift_bind。
- 制造后房间受前房影响：查制造房提交顺序和每房 resolve_base，不能恢复共享静态 top_k。
- 多班成员拆开：查 AssignmentPlan bind → shift_binds_from_plan → align_shift_binds_in_halves → schedule invariant。
