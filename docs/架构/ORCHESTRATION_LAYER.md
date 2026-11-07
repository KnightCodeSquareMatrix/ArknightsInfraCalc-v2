# 编排层

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/架构/SYSTEM_CHAINS.md；docs/排班/定时换班.md；docs/排班/自动轮换.md
> 摘要：说明声明式规则如何编译为 AssignmentPlan 并交给 executor
> 代码范围：crates/infra-core/src/layout/orchestrate/、layout/assign/、data/orchestration_rules.json
> 知识库桥接：docs/机制/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：build_plan、execute_plan、SystemAnchor、AssignmentPlan
> 关联链路：docs/架构/BASE_ASSIGNMENT.md、docs/架构/SYSTEM_CHAINS.md、docs/模块/排班与轮换.md
> 核验基线：Plan owner、resolved dependency、execute 后 BaseAssignment

编排层把声明式 system/alternative 编译成有限 Plan，再由 executor 执行；负责 required admission、scope、anchor、dependency 和 Plan owner。设施 solver 负责房间效率，schedule 负责跨班轮换。

    rules / knowledge refs
      → build_plan（有限 alternatives）
      → execute_plan（anchor/dependency）
      → assign_shift / fill
      → AssignmentPlan + BaseAssignment

build_plan 解析 PickOne、OptionalFixed、required count、scope 和 alternatives，不写最终房间。execute_plan 消费 resolved facts，提交 cross-station/same-station claims 和 dependency。shift_bind 只能消费 Plan 实际 bind；tag、priority、shortcut 不能替代 required admission。

当前策略是跨站体系先于同站候选、producer/consumer 以实际 presence 形成 dependency、制造逐房重算 resolve_base、fallback 只限声明式 role/alternative。旧路线图、删除 helper 和历史审计仅在 ARCHIVE 追溯。

制造体系试排按每个临时 assignment 逐房 `resolve_base`，直接调用实时制造搜索；请求级完整精确键 memo 保留。额外条件响应表已删除，不能恢复不同上下文共享的静态 top-K；取舍与范围见 [搜索性能与 Bake 边界](../质量与维护/PERFORMANCE_ENGINEERING.md#制造体系试排的缓存取舍2026-10-31)。

## 变更时的调用链定位

回收 `recycle_skilled` 在普通设施填充前进入 Plan，`recycle_objective` 按实际解锁回收技能效率排序并保留最多两席，同房成员通过 Plan 绑定轮换。ABC 中没有生产或中枢成员可继承的独立绑定组，按实际累计工作窗口最大的 Team 归属，平局保留首标签；归属由共同绑定 helper 生成，不能静默漏掉 anchors。该规则属于“回收技能优先于普通发电补位”的 policy，不换算两个设施的产出，也不承诺全基建最优。莱茵支持角色允许使用同一宿舍的多个空床；容量、回收技能预留、实际 receiver 赢家与整条 alternative 的提交/回退继续由原编译执行 owner 负责。

- 组合未进入岗位：先查 build_plan 是否有 required admission/alternative，再查 execute_plan 是否提交 anchor；不要先调 facility solver。
- 组合进入但收益为零：查 runtime capability、scope 和实际同房/同班 presence，再查 resolve_base；不要按名字增加 shortcut。
- 跨房 consumer 缺 producer：查 resolved_producer_dependencies 是否产生 exact_presence/requires_presence；没有 dependency 时 schedule 不应生成 shift_bind。
- 制造后房间受前房影响：查制造房提交顺序和每房 resolve_base，不能恢复共享静态 top_k。
- 多班成员拆开：查 AssignmentPlan bind → shift_binds_from_plan → align_shift_binds_in_halves → schedule invariant。
