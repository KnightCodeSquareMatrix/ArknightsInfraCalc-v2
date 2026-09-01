# 架构导览：从 `plan` 到 MAA 排班

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/ORCHESTRATION_LAYER.md；docs/BASE_ASSIGNMENT.md；docs/INFRA_CLI.md；docs/排班模式.md
> 摘要：解释当前核心调用链和模块边界

> 实现快照：Current。本文只解释已经存在的调用链；历史计划和未来候选架构不属于当前运行时。

本文适合第一次追踪完整排班结果的开发者。先由 [AGENTS.md](../AGENTS.md) 完成任务分类；Debug 时读 [MAINTENANCE_MODE.md](MAINTENANCE_MODE.md)，代码 owner 不明时再定向查 [PROJECT_MAP.md](PROJECT_MAP.md)。如果只想理解基建业务过程而不关心代码入口，读 [GONGSUN_RUNTIME_OVERVIEW.md](GONGSUN_RUNTIME_OVERVIEW.md)。

## 1. 先分清事实来源与代码层

| 层 | 当前职责 | 主要入口 |
|---|---|---|
| 当前用户裁决 | 当前对话中明确补充或纠正的业务口径；确认后应同步到对应 canonical | [AGENTS.md](../AGENTS.md)、[SYSTEM_AUDIT_WORKFLOW.md](SYSTEM_AUDIT_WORKFLOW.md) |
| 领域 canonical | 每个领域键的唯一当前规范；代码、数据和旧测试不能反推或推翻它 | [INDEX.md](INDEX.md) 生成的 canonical 表 |
| 代码 / help | 证明当前实现 owner、入口和接口事实，不反向裁决业务语义 | [PROJECT_MAP.md](PROJECT_MAP.md)、[INFRA_CLI.md](INFRA_CLI.md) |
| `data/` | 技能、干员实例、声明式编排规则、兼容体系、shortcut、布局和回归锚点的运行时载体 | `operator_instances.json`、`skill_table.json`、`orchestration_rules.json`、`base_systems.json`、`trade_shortcuts.json` |
| L1 | 把 `buff_id` 解释为 EffectAtom 行为；解释器不认识干员名 | [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md)、[trade/interpreter.rs](../crates/infra-core/src/trade/interpreter.rs)、[manufacture/interpreter.rs](../crates/infra-core/src/manufacture/interpreter.rs) |
| L2 | 处理不能只靠局部 atom 表达的机制域求解 | [gold_flow.rs](../crates/infra-core/src/trade/gold_flow.rs)、[order_mechanic.rs](../crates/infra-core/src/trade/order_mechanic.rs)、[unit_output.rs](../crates/infra-core/src/trade/unit_output.rs) |
| L3 | 对固定最优或难 atom 化的贸易组合做 shortcut 结算 | [shortcut.rs](../crates/infra-core/src/trade/shortcut.rs)、[segment.rs](../crates/infra-core/src/trade/segment.rs)、[INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md) |
| GL | 生成并消费跨设施资源、全局注入和 scope=Global atom | [control/](../crates/infra-core/src/control/)、[global_resource/](../crates/infra-core/src/global_resource/)、[cross_facility/](../crates/infra-core/src/cross_facility/)、[INTERNAL/CROSS_FACILITY.md](INTERNAL/CROSS_FACILITY.md) |
| Layout | Rule/兼容 System 编译、Plan 落位、设施补位、`used` 互斥、全基建上下文和轮换 | [BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md)、[ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)、[排班模式.md](排班模式.md) |
| Scoring | 各生产域保存独立直接效率；中枢局部 heuristic 使用具名 policy | [EFFICIENCY_MODEL.md](EFFICIENCY_MODEL.md)、[SCORING_MODEL.md](SCORING_MODEL.md) |
| CLI | transport/I/O adapter；共享 Plan 计算 owner 是 `plan_compute.rs`，adapter 不实现业务公式 | [INFRA_CLI.md](INFRA_CLI.md)、[commands/plan_compute.rs](../crates/infra-cli/src/commands/plan_compute.rs) |

最重要的边界是：用户裁决与领域 canonical 决定“应该是什么”，代码/help 证明“当前怎样实现”，`data/` 描述“运行时拿什么解释”，core 决定“如何求解”，CLI adapter 只决定“如何调用和输出”。

## 2. 完整主线

```text
argv `plan` ───────────────┐
Worker `plan.compute` ─────┼─→ commands/plan_compute.rs::compute_plan
legacy Worker `plan` ──────┘
                                │
                                └─ run_user_rotation_probe_with_profile_and_preferences
                                     └─ schedule_timed_rotation
                                          └─ selected profile assignment path
                                               ├─ build_plan
                                               └─ run_shift_pipeline
                                                    ├─ execute_plan
                                                    ├─ build facility pools
                                                    ├─ facility fill / search
                                                    └─ repeated resolve_base snapshots
                                │
                                ├─ current.rotation ─→ text / rotation summary
                                ├─ build_box_profile_from_current_probe ─→ profile JSON
                                └─ build_from_team_rotation ─────────────→ MAA JSON
```

三个 adapter 只改变输入协议和输出形态；单次请求只计算一次用户 rotation，adapter 不会为不同输出另行重跑它。`resolve_base` 也不是只在末尾调用一次：填房阶段需要不断用当前部分编制重建跨设施上下文，多班生成后又会按每班实际人员重新结算。

## 3. 三个 adapter 与共享 `compute_plan`

[commands/plan.rs](../crates/infra-cli/src/commands/plan.rs) 的 argv `plan`、[commands/serve.rs](../crates/infra-cli/src/commands/serve.rs) 的内联 `plan.compute` 和 legacy Worker `plan` 都汇合到 [commands/plan_compute.rs](../crates/infra-cli/src/commands/plan_compute.rs) 的 `compute_plan`：

1. adapter 校验 argv、文件路径或内联 JSON，并准备 `BaseBlueprint`、`OperBox`、instances 与 skill table。
2. `compute_plan` 调用一次 `run_user_rotation_probe_with_profile_and_preferences`；该 probe 通过所选 `TimedRotationProfile` 进入 `schedule_timed_rotation`。
3. 请求 profile 时，`build_box_profile_from_current_probe` 消费该 probe，不再运行第二次用户 rotation；profile 仍独立评估固定 baseline。
4. 请求 MAA 时，`build_from_team_rotation` 消费同一 `current.rotation`。
5. argv `plan` 用同一 rotation 打印文本并总是写 profile；`plan.compute` 返回 profile、rotation summary、MAA，并以同一请求的 operbox/layout 附加结构化练卡建议；legacy Worker `plan` 返回 rotation 摘要，并按请求生成文件输出。

profile 是同一次排班 probe 的分析投影，不包含完整 MAA assignment。独立调用 `build_box_profile` 仍是合法 library 入口，但它不是用户 `plan` 的共享计算路径。

## 4. OperBox：账号拥有与练度进入求解器

[operbox/mod.rs](../crates/infra-core/src/operbox/mod.rs) 保存玩家拥有的姓名、精英阶段、等级和稀有度；[operbox/xlsx.rs](../crates/infra-core/src/operbox/xlsx.rs) 负责一图流导入。

OperBox 本身不保存完整技能公式。建池时由 `OperatorInstances` 按练度解析设施绑定和 `buff_id`：

```text
OperBoxEntry
  + operator_instances.json 的 tier/facility 绑定
  + skill_table.json 的 buff_id → atoms
  = ControlPool / TradePool / ManuPool / PowerPool entry
```

因此排查“账号有干员但没进池”时，应依次检查姓名匹配、练度 tier、设施绑定、`buff_id` 和池过滤，而不是先改搜索排序。

## 5. `build_plan`：结构选型 + late competitive 比较，输出统一 Plan

`schedule_timed_rotation` 先分派具名 profile；需要构造 Peak 的路径会调用 `assign_shift_with_plan_skip`，后者在 [layout/assign.rs](../crates/infra-core/src/layout/assign.rs) 中先调用 [orchestrate/select.rs](../crates/infra-core/src/layout/orchestrate/select.rs) 的 `build_plan`。默认 ABC 经由该路径生成 peak assignment 与 plan。

`build_plan` 按唯一优先级顺序合并三段来源：

1. 先编译 `orchestration_rules.json` 中 `priority >= 19` 的声明式 Rule；
2. 再选择 `base_systems.json` 的兼容 registry claims，且只看前段已解析后的真实占位、排除与容量；
3. 最后编译 `priority <= 18` 的 late Rule，读取前两段落位后的真实空位。

输出统一的 `AssignmentPlan`，包含：

- `selected_rules`、`registry_claims` 与已激活体系；
- 已经解析到实际房间的 required anchors；
- optional producers；
- 同房/禁同房等 constraints；
- 路径级 `excluded_operators`；
- degradation 信息；
- shift binds；
- actual active dependencies 与 continuous roles；
- 中枢搜索候选要求。

高优先硬规则只按结构不变量解析 required core，不靠 solver 决定硬核心；late competitive Rule 可调用已有制造/发电 domain solver，与普通基线做 Pareto 比较，resource gate 也可读取候选 assignment 的 `resolve_base` 结果，但规则层不复制机制公式。required anchor、`shift_bind` 和 shortcut 也不是同一种东西：anchor 保证进编，bind 约束已进编人员的跨班关系，shortcut 只负责最终组合结算。

更详细的不变量见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md) 和 [BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md)。

## 6. `execute_plan` 与 `used`：把计划落成初始编制

[orchestrate/execute.rs](../crates/infra-core/src/layout/orchestrate/execute.rs) 的 `execute_plan` 接受 seed assignment 和 `AssignmentPlan`，直接落位全部 resolved anchors 与兼容 registry claims，并把 `excluded_operators` 扩展进 `used`，返回：

- 已经落位的 `BaseAssignment`；
- 同步的全局 `used: HashSet<String>`。

随后 [layout/assign/pipeline.rs](../crates/infra-core/src/layout/assign/pipeline.rs) 直接消费这份已落位 assignment、`used` 与 plan 约束，继续设施建池、补位和 `resolve`；不再二次放置另一套代码化 anchors。`used` 是跨设施人员互斥的运行时事实；任何提前落位或路径级排除都会改变后续设施的可选池。

这也是编排 bug 不能在最下游“塞一个人”的原因：若硬核心应在 `AssignmentPlan` 表达，却只在 fill 阶段补人，其他设施已经可能抢走该干员或其队友。

## 7. Pipeline：建池、搜索、补位与反复 resolve

[layout/assign/pipeline.rs](../crates/infra-core/src/layout/assign/pipeline.rs) 是单班阶段顺序的事实源。Peak 主路径当前执行：

1. `execute_plan` 一次性落位 plan 中的 resolved anchors 与 registry claims，并合并 exclusions 到 `used`。
2. 从 operbox 建 control/trade/manufacture/power 四类池。
3. 把 plan tier、候选要求、constraints 与已解析 anchor 的搜索元数据注入池。
4. 比较普通高峰前缀与当前登记的可选动态贸易 producer 前缀。
5. 每个前缀依次处理中枢、体系 producer、宿舍 producer、发电和贸易；阶段间通过 `resolve_snapshot` 获取新上下文。
6. 只让胜出的前缀进入制造站填充。
7. 对最终制造、贸易房间刷新效率快照。

Recovery 有独立顺序，不能用 Peak 的假设推断。各设施的具体补位代码位于 `layout/assign/*_fill.rs`。

搜索层负责枚举候选并按本域口径排序：

- 贸易：[search/trade.rs](../crates/infra-core/src/search/trade.rs)；
- 制造：[search/manufacture.rs](../crates/infra-core/src/search/manufacture.rs)；
- 中枢：[search/control.rs](../crates/infra-core/src/search/control.rs)；
- 发电：[search/power.rs](../crates/infra-core/src/search/power.rs)。

`top_k` 是保留/返回候选的上限，不应被误解为业务规则。完整池与 standalone 池的安全边界见 [PERFORMANCE_ENGINEERING.md](PERFORMANCE_ENGINEERING.md)。

## 8. `resolve_base`：从人员名单构造全基建上下文

[layout/resolve.rs](../crates/infra-core/src/layout/resolve.rs) 的 `resolve_base` 接受 blueprint 与当前 assignment：

1. `WorkforceIndex::build` 建立按房间和设施划分的人员索引。
2. 从蓝图写入设施数、设施等级、宿舍规划、配方种类和初始全局资源。
3. 写入全基建人员名单、跨设施 faction/tag 计数。
4. 依次应用 power、control 和 office 对 `LayoutContext` 的影响。
5. 收集并执行 scope=Global atom。
6. 执行全局资源 conversions。
7. 为贸易、制造和发电房间构造带各自 layout 快照的 resolved room input。

`resolve_base` 负责上下文与房间输入，不替代各房 solver。贸易搜索或最终刷新仍会把 `ResolvedTradeRoom` 交给 `solve_trade_with_shift`；制造同理。

## 9. 单房求解：L1 → L2 → L3

贸易房最终进入 [trade/solver.rs](../crates/infra-core/src/trade/solver.rs)：

```text
TradeRoomInput
  → interpreter：按 Phase 执行 Condition / Selector / Action，并处理 gold_flow 状态
  → 金单尝试 L3 segment + shortcut
       ├─ 命中：shortcut 构造 order mechanic
       └─ 未命中：L2 order_mechanic 正常求解
  → unit_output
  → TradeResult.final_efficiency
```

L3 不是候选选型器。它只在真实三人组和 producer manifest 已确定后判断是否命中社区锚点。贸易内部函数导航见 [INTERNAL/TRADE_INTERPRETER.md](INTERNAL/TRADE_INTERPRETER.md) 与 [INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md)。

制造站使用自己的 interpreter/solver，不复用贸易假设，见 [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md)。

## 10. Scoring：生产效率分域，中枢 heuristic 具名

生产域没有匿名总分：

- 贸易按 `final_efficiency`；
- 制造按 `final_efficiency`；
- 发电按 `final_efficiency`；
- timed rotation 日报分别汇总 trade/manufacture/power，不相互混加。

中枢搜索当前使用 `ControlInjectRawSumV0` 局部 policy。它是对贸易/制造注入分量的具名排序 heuristic，不是全基建产出函数，也不能冒充贸易最终效率。数值定义见 [EFFICIENCY_MODEL.md](EFFICIENCY_MODEL.md) 与 [SCORING_MODEL.md](SCORING_MODEL.md)。

## 11. `schedule_timed_rotation`：分派具名定时轮换

[schedule/team_rotation.rs](../crates/infra-core/src/schedule/team_rotation.rs) 以闭集 profile 分派默认 ABC `12/6/6`、ABC `12/12/12`、二班 `12/12`、菲亚 `8/8/4/4` 和深海 `7/5/7/5`。`schedule_team_rotation` 只是默认 ABC 的薄包装；下面步骤描述 ABC 路径：

独立求解的主力/替补各自保留 `AssignmentPlan`，最终 `shifts[].plan_index` 指向真实 owner。CLI 和 MAA 不参与 profile 选型或机制重算。

1. 先通过完整单班 pipeline 得到 peak assignment 与 peak plan。
2. 根据实际 peak 编制派生 shift binds。
3. 提取宿舍共享脚手架；办公室不共享、不进入生产 cohort，在每班生产和固定岗位完成后从剩余候选中择优。
4. 把 peak 生产设施切成 alpha/beta 两个半区。
5. 从剩余池构建 gamma 对两个半区的替补。
6. 按每班活跃队伍重建中枢和各生产房间。
7. 对三班实际 assignment 重新评分，处理菲亚梅塔回岗。
8. final validator 显式校验中枢 5 人、bind 次数/共同 presence 与计划声明的依赖事实；可选动态 producer 按每班实际在场结算，不再按标签重推；生产房容量和人员互斥继续由构造、`used` 与回归验收。
9. 分别汇总三类按时长加权的 daily totals。

轮换不等于连续心情全局最优化；当前 profile 与导出契约见 [排班模式.md](排班模式.md) 和 [SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md)。

## 12. MAA：导出已经求解的排班，不重新选人

[export/maa.rs](../crates/infra-core/src/export/maa.rs) 的 `build_from_team_rotation` 遍历报告中的全部班次：

- 把每班 `BaseAssignment` 映射到 MAA 房间槽位；
- 写入班次名、时长说明、休息队和无人机默认配置；
- 使用 rotation 已确认的菲亚梅塔动作；
- 输出 `MaaSchedule`，由 CLI 保存到 `--maa-out`。

MAA 导出层不运行机制公式，也不应该修正上游非法编制。若 JSON 人员不对，应先检查 rotation assignment；若 assignment 正确但字段形状不对，才检查 exporter。

## 13. 按症状定位

| 症状 | 先看 |
|---|---|
| 干员未进入可选池 | OperBox → instances tier → pool builder |
| 体系未激活或硬核心缺失 | `build_plan`、`AssignmentPlan`、对应体系 Markdown |
| 干员被别的设施抢走 | `execute_plan`、pipeline 阶段顺序、`used` |
| 单房效率错误 | room input → L1/L2/L3；不要先改 CLI |
| 多房选择不理想 | facility fill、role/候选过滤、搜索排序与上下文刷新 |
| 多班同上同下或轮休错误 | actual shift binds、timed rotation invariant validation |
| MAA 字段错误但 assignment 正确 | `export/maa.rs` |

任务先由 [AGENTS.md](../AGENTS.md) 分类；maintenance 的完整复现、修改门禁和验证留痕要求见 [MAINTENANCE_MODE.md](MAINTENANCE_MODE.md)。
