# 术语表

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md；docs/MAINTENANCE_MODE.md；docs/QUALITY_AND_AUDIT.md；docs/EFFECT_ATOM_DESIGN.md；docs/EFFICIENCY_MODEL.md；docs/SCORING_MODEL.md；docs/ORCHESTRATION_LAYER.md；docs/BASE_ASSIGNMENT.md；docs/排班模式.md；docs/定时换班.md；docs/SCHEDULE_ROTATION.md
> 摘要：以短定义把项目术语路由到各 canonical 或 current owner

> 实现快照：Current
> 读者：所有文档读者与协作者
> 本文负责：中文业务词与实际类型、字段或 ID 的对照导航；不拥有领域键，不裁决具体体系业务规则

本文只提供足以辨认 owner 的短定义。与本文冲突时，业务语义服从用户当前裁决和对应 canonical，当前实现事实以代码与生成 help 为准。反引号内保留实际类型、字段或稳定 ID。

## 文档、任务与证据

| 术语 | 短定义 | Owner / 详见 |
|---|---|---|
| canonical | 拥有一个或多个稳定领域键并裁决当前规范的文档；每个领域键恰好一个 owner。 | [文档生命周期](文档生命周期.md) |
| current-reference | 解释当前实现、导航或工作流的参考文档；可以失真，不能反向推翻 canonical。 | [文档生命周期](文档生命周期.md) |
| active-change | 位于 `docs/TODO/`、确需跨会话继续的提案或开放任务；未恢复时不自动执行。 | [文档生命周期](文档生命周期.md) |
| decision / ADR | 保存持久架构决策、理由和替代关系；current facts 仍由所链接的 current owner 维护。 | [文档生命周期](文档生命周期.md) |
| generated-reference | 由确定性 generator 重建的事实；标记区不得手工维护第二份内容。 | [文档生命周期](文档生命周期.md) |
| archive | 完成、被替代或历史材料；只能解释历史，不能证明当前行为。 | [文档生命周期](文档生命周期.md) |
| 真源 / source of truth | 业务裁决顺序是用户明确裁决、领域 canonical、代码/help 实现事实、数据与测试载体。 | [AGENTS.md](../AGENTS.md#2-真源与任务路由) |
| maintenance / 维护型任务 | 复现并修复 bug、错误结果或既有行为一致性的工作流，不是项目生命周期阶段。 | [Debug 指南](MAINTENANCE_MODE.md) |
| `local` / local repair | 正确 owner 已能表达规则，只在 owner 内修正。 | [AGENTS.md](../AGENTS.md#2-真源与任务路由)、[Debug 指南](MAINTENANCE_MODE.md#11-任务与改动形态) |
| `conformance-rebuild` | 现有模型无法表达已确认不变量，或多个阶段重复兜底时，重建单一责任边界。 | [AGENTS.md](../AGENTS.md#2-真源与任务路由)、[Debug 指南](MAINTENANCE_MODE.md#11-任务与改动形态) |
| `formal-audit` | 用户要求严格逐项审计或两个 current canonical 冲突时启用的审计模式。 | [AGENTS.md](../AGENTS.md#2-真源与任务路由)、[体系审计工作流](SYSTEM_AUDIT_WORKFLOW.md) |
| 验证留痕 | 按任务风险选择是否用 `run_evidence.sh` 保存命令、输入、stdout/stderr、exit code 和产物到 `target/codex-runs/<task>/`；不是所有任务的硬门禁。 | [质量与证据总则](QUALITY_AND_AUDIT.md#4-验证留痕按需) |

## 输入、布局与运行时对象

| 术语 | 短定义 | Owner / 详见 |
|---|---|---|
| 练度盒 / operbox | 玩家拥有的干员及精英化、等级、潜能等输入，决定候选存在与技能解锁档。 | `infra-core::operbox`；[项目地图](PROJECT_MAP.md) |
| roster | 为某设施构造搜索池的名单，可来自 operbox 或维护 CSV；不是最终上岗名单。 | `infra-core::roster` / `pool` |
| standalone roster / 工具人池 | 在明确领域中缩小自由搜索空间的具名高价值散件目录；不是技能语义真源。 | [性能工程](PERFORMANCE_ENGINEERING.md)、`pool/standalone.rs` |
| `full_pool` | 对当前设施和输入满足合法门禁的完整普通候选池；不能由 standalone 名录替代。 | [制造状态](MANUFACTURE_STATUS.md)、[质量总则](QUALITY_AND_AUDIT.md#33-layout-生命周期层) |
| `BaseBlueprint` / 蓝图 | 房间结构、设施类型、等级、容量和配方；描述岗位，不描述谁上岗。 | [单班编制](BASE_ASSIGNMENT.md)、`layout/blueprint.rs` |
| `BaseAssignment` / 编制 | `room_id -> operators` 的实际落位结果；同一班内同一干员至多上岗一次。 | [单班编制](BASE_ASSIGNMENT.md)、`layout/assignment.rs` |
| `RoomAssignment` | `BaseAssignment` 中一间房的成员和可选效率快照。 | `layout/assignment.rs` |
| `LayoutContext` | 从蓝图和当前编制派生的全基建只读上下文。 | `layout/context.rs` |
| `SharedLayout` | `Arc<LayoutContext>` 的共享别名，供搜索热路径复用。 | `layout/context.rs` |
| `ResolvedBase` | `resolve_base` 的完整派生结果；生产房真实分数仍由对应 solver/search 结算。 | `layout/resolve.rs` |
| `WorkforceIndex` | 按房间/设施索引实际进驻成员并投影 workforce、tag 和跨设施统计。 | `layout/workforce.rs` |
| workforce | 当前实际在基建或某设施上岗的成员集合；`base_workforce`、trade/control workforce 作用域不同。 | [EffectAtom](EFFECT_ATOM_DESIGN.md)、`layout/workforce.rs` |
| `room_id` | 蓝图、编制、输出和 MAA 共用的稳定房间标识；样例房号不能写成业务公式。 | [单班编制](BASE_ASSIGNMENT.md) |

## 练度、技能与机制分层

| 术语 | 短定义 | Owner / 详见 |
|---|---|---|
| `PromotionTier` | 技能解锁档，运行时主要区分 `tier_0` 与 `tier_up`；不是编排优先级。 | `tier.rs`、`instances.rs` |
| `OperatorTier` | 编排候选层级 `CrossStation` / `SameStation` / `Standalone`；不是精英化等级。 | [编排层](ORCHESTRATION_LAYER.md#2-三种耦合范围--三种职责不混层)、`layout/tier.rs` |
| `buff_id` | 游戏解包技能 ID，也是 `skill_table.json` 的结构键。 | [EffectAtom](EFFECT_ATOM_DESIGN.md)、`skill_table.rs` |
| `operator_instances.json` | 干员、练度档、设施和 `buff_id` 的运行时归属映射。 | `instances.rs` |
| `skill_table.json` | `buff_id -> SkillDef / EffectAtom[]` 的运行时机制表。 | `skill_table.rs`、[EffectAtom](EFFECT_ATOM_DESIGN.md) |
| `EffectAtom` | selector、action、可选 condition、phase、tag 和 scope 组成的平坦机制单元。 | [EffectAtom](EFFECT_ATOM_DESIGN.md) |
| `Selector` / `Action` / `Condition` | atom 的取数方式、操作和触发门禁。 | [EffectAtom](EFFECT_ATOM_DESIGN.md) |
| `Phase` / `phase_order` | atom 的稳定执行阶段，以及同一阶段内的确定次序。 | [EffectAtom](EFFECT_ATOM_DESIGN.md) |
| `AtomScope::Room` / `Global` | atom 只在当前房间解释，或由 `cross_facility` 在全基建上下文执行。 | [EffectAtom](EFFECT_ATOM_DESIGN.md) |
| `atoms: []` | 技能已登记但委托给 L2/L3 或其他领域引擎；不自动表示未建模。 | [EffectAtom](EFFECT_ATOM_DESIGN.md) |
| tag | 在明确文件/域中使用的结构标记；其作用域必须由 owner 定义，不能代替 required admission。 | [AGENTS.md](../AGENTS.md#3-所有任务通用硬门禁)、对应领域 canonical |
| L1 / interpreter | 按 Phase 解释通用 EffectAtom；只认 `buff_id`，不按干员名写公式。 | [EffectAtom](EFFECT_ATOM_DESIGN.md)、对应 interpreter |
| L2 / 域引擎 | 处理不能由独立 atom 正确表达的领域机制，如 gold flow、订单分布和单位产出。 | 对应领域 canonical 与模块 |
| L3 / shortcut | 根据实际组合和上下文命中表化结算；不负责体系选型或进编。 | [Shortcut 地图](INTERNAL/SHORTCUT_MATCHING.md) |
| shortcut | `trade_shortcuts.json` 中的实际组合结算条目，通常带稳定 `rule_id`。 | [Shortcut 地图](INTERNAL/SHORTCUT_MATCHING.md) |
| segment / 链段 | `trade_segments.json` 中 producer 前提、consumer 类型、shortcut 和 role fallback 的连接。 | [Shortcut 地图](INTERNAL/SHORTCUT_MATCHING.md) |
| `rule_id` | 本次 L3/社区规则命中的稳定标识；`None` 表示未命中 active shortcut。 | `trade/shortcut.rs`、`infra-cli verify` |
| producer / consumer | 产生前提、资源或注入的成员/设施，以及读取该结果的房间/成员；关系范围由领域 owner 裁决。 | [中枢规则](CONTROL_CENTER_ASSIGNMENT.md)、`response_dependency.rs` |
| `GlobalResourcePool` | 感知、人间烟火、木天蓼、魔物料理、虚拟发电等具名全局资源池。 | [EffectAtom](EFFECT_ATOM_DESIGN.md)、`global_resource/pool.rs` |
| `GlobalInjectManifest` | 中枢等设施向生产房写入的全局效率规则和 producer gate；不是可消耗资源池。 | `global_resource/inject.rs`、[中枢规则](CONTROL_CENTER_ASSIGNMENT.md) |
| `cross_facility` | 收集 `scope=Global` atom，执行资源生产/转化并形成全基建资源快照。 | [跨设施地图](INTERNAL/CROSS_FACILITY.md) |
| conversion / 全局转化 | 具名资源之间的转换；共享读取不能被当成一次性扣减。 | [EffectAtom](EFFECT_ATOM_DESIGN.md)、`global_resource` |

## 搜索、保证与编排

| 术语 | 短定义 | Owner / 详见 |
|---|---|---|
| pool / 候选池 | 按设施、练度、技能和当前约束构造的可搜索成员集合。 | `pool/`、对应设施 canonical |
| candidate / 候选 | 尚未提交的合法成员组合或编排方案，可携带 role、metric 和规则 ID。 | `candidate.rs`、对应 search/orchestrate owner |
| hit / search hit | 已由单房 solver 求值的候选结果，如 `TradeSearchHit`。 | `search/` |
| `top_k` | 报告保留的前 K 个结果；不是硬业务规则，也不能证明完整性。 | [质量总则](QUALITY_AND_AUDIT.md#21-搜索空间与保证等级) |
| 自然搜索 | 在硬约束后的合法空间按正式 solver 结果选择，不用名字顺序、固定分或样例特判强塞。 | 对应领域 canonical、[质量总则](QUALITY_AND_AUDIT.md) |
| `hard_constraint` | 违反 canonical 即非法的候选门禁。 | [质量总则](QUALITY_AND_AUDIT.md#21-搜索空间与保证等级) |
| `safe_reduction` | 有证明不删除最优解的 dominance、symmetry 或 bound。 | [质量总则](QUALITY_AND_AUDIT.md#21-搜索空间与保证等级) |
| `search_heuristic` | 只改变搜索顺序或资源分配，exact 路径仍可恢复完整候选。 | [质量总则](QUALITY_AND_AUDIT.md#21-搜索空间与保证等级) |
| `policy_restriction` | 用户具名限制可行空间或取舍；保证只适用于该 policy。 | [质量总则](QUALITY_AND_AUDIT.md#21-搜索空间与保证等级) |
| `approximation` | 为时间/内存主动放弃完整性，必须报告停止原因和 fallback。 | [质量总则](QUALITY_AND_AUDIT.md#21-搜索空间与保证等级) |
| 结果状态 | `EXACT_OPTIMAL`、`POLICY_OPTIMAL`、`BEST_FOUND`、`UNKNOWN/TIME_LIMIT`、`INFEASIBLE` 等保证结论。 | [质量总则](QUALITY_AND_AUDIT.md#21-搜索空间与保证等级) |
| System | 可被编排选择的结构统称；当前复杂路径由 Rule alternatives 表达，`base_systems.json` 只保留 legacy registry。 | [编排层](ORCHESTRATION_LAYER.md) |
| Rule / alternative | `orchestration_rules.json` 中声明的 gate、role、relation 和有限备选方案。 | [编排层](ORCHESTRATION_LAYER.md#4-通用规则-schema) |
| `AssignmentPlan` | 已解析的可执行计划，保存 placement、anchor、bind、dependency 和 reserve。 | [编排层](ORCHESTRATION_LAYER.md)、`layout/orchestrate/plan.rs` |
| `build_plan` | 从规则、legacy registry、operbox、蓝图和偏好生成 resolved Plan。 | `layout/orchestrate/select.rs`、[编排层](ORCHESTRATION_LAYER.md) |
| `execute_plan` | 把 Plan 中已解析 placement 写入编制和 `used`；不重新选型。 | `layout/orchestrate/execute.rs`、[编排层](ORCHESTRATION_LAYER.md) |
| fill | 对尚未确定的中枢、producer、发电、贸易和制造位置执行各域搜索。 | `layout/assign/pipeline.rs`、[单班编制](BASE_ASSIGNMENT.md) |
| resolve | 从当前蓝图和编制重新计算 workforce、资源、注入和 resolved rooms。 | `layout/resolve.rs`、[单班编制](BASE_ASSIGNMENT.md) |
| pipeline | Layout 主路径是 `build_plan -> execute_plan -> fill -> resolve`；rotation/export 是下游消费者。 | [AGENTS.md](../AGENTS.md#4-核心分层边界)、[项目地图](PROJECT_MAP.md#关键生产链) |
| anchor | Plan 中提前解析的必要落位；通常只固定必要成员，其余位置仍由搜索补齐。 | [编排层](ORCHESTRATION_LAYER.md) |
| required anchor / admission | 体系激活后必须实际进编的成员/placement；tag、priority、shortcut 或 `shift_bind` 不能替代。 | [编排层](ORCHESTRATION_LAYER.md)、对应体系 canonical |
| bond | 明确要求同房的固定核心关系，通常固定必要成员后再搜索剩余位置。 | [编排层](ORCHESTRATION_LAYER.md) |
| role / core priority | 保留核心和 fallback 候选边界的领域 policy；不等于固定三人组。 | [编排层](ORCHESTRATION_LAYER.md)、`search/role_pick.rs` |
| degradation / fallback | owner 已定义的较弱合法路径；不能通过吞错或下游强塞伪造。 | 对应领域 canonical |
| `used` / commit | 当前班已占用成员集合，以及将候选写入 assignment 并同步占用状态的动作。 | [单班编制](BASE_ASSIGNMENT.md)、`layout/assign/commit.rs` |

## 排班模式、队伍与班次

| 术语 | 短定义 | Owner / 详见 |
|---|---|---|
| `auto_rotation` | 游戏内置按组合耗尽触发的自动轮换；当前仍待独立设计。 | [排班模式](排班模式.md) |
| `timed_rotation` | MAA 或人工按固定时间/间隔执行完整班次表；当前默认模式。 | [排班模式](排班模式.md)、[定时换班](定时换班.md) |
| `mower_dynamic` | Mower 按实时心情动态决定换班、宿舍和替补；当前暂停。 | [排班模式](排班模式.md) |
| rotation profile | timed rotation 的闭集策略；当前实现含 ABC、主备 12/12、主备 24/24、菲亚二班/四班和深海四班。 | [排班轮换](SCHEDULE_ROTATION.md#1-当前模型) |
| Team / 队伍 | 共享出勤节奏的一组成员或房间，如 α/β/γ；不是某个时刻的完整编制。 | [定时换班](定时换班.md)、[排班轮换](SCHEDULE_ROTATION.md) |
| Shift / 班次 | 一个持续时间明确、实际在岗 assignment 完整的状态。 | [定时换班](定时换班.md)、[排班轮换](SCHEDULE_ROTATION.md) |
| Peak | `AssignShiftMode` 当前唯一的取值，表示单班主力编制；不是 timed rotation profile 名。早期的 Recovery 模式已删除。 | [单班编制](BASE_ASSIGNMENT.md)、`layout/shift.rs` |
| αβγ ABC | 默认 `abc_12_6_6` profile 的三队拓扑；不是全部 rotation profile 或全部排班模式。 | [排班轮换](SCHEDULE_ROTATION.md#11-默认-abc)、[排班模式](排班模式.md) |
| H1 / H2 | 默认 ABC 把生产设施划分的两个稳定半区。 | [排班轮换](SCHEDULE_ROTATION.md#2-abc-轮换流程) |
| rotation/bind cohort | 因 Team presence 或 exact bind 必须共享出勤节奏的成员/房间集合。 | [排班轮换](SCHEDULE_ROTATION.md#5-班次绑定shift_bind) |
| 贸易 A/B/C cohort | 完整 Rosemary alternative 下的但书、可露黑键、龙巫三组贸易核心；只属于该条件路径。 | [排班轮换](SCHEDULE_ROTATION.md#4-243-贸易-core-role) |
| `shift_bind` | 约束已经入选成员同队、同上同下或 presence；不负责第一次进编。 | [排班轮换](SCHEDULE_ROTATION.md#5-班次绑定shift_bind) |
| presence vector | 某成员在各 Shift 是否实际上岗的布尔向量，用于验证 producer/consumer 与 bind。 | [排班轮换](SCHEDULE_ROTATION.md#5-班次绑定shift_bind) |
| α / β / γ | 默认 ABC 中的三个 Team；α、β 来自 peak 两半，γ 是替补队。 | [排班轮换](SCHEDULE_ROTATION.md#2-abc-轮换流程) |
| mood ETA | 根据当前净心情消耗估算主力可持续时间的锚点；不会自动改写固定 profile。 | [排班轮换](SCHEDULE_ROTATION.md#71-peak-主力最长工作时间) |
| 菲亚覆盖 / return | 部分 profile 中按固定班次生成的具名换人事件；具体次数、目标和状态责任见 canonical 规则。 | [菲亚梅塔使用规则](菲亚梅塔使用规则.md)、[排班轮换](SCHEDULE_ROTATION.md) |
| MAA JSON | 从最终班次 assignments 映射出的 MAA 基建协议；export 不重新计算机制。 | [定时换班](定时换班.md)、`export/maa.rs` |

## 效率、产出与评分

| 术语 | 短定义 | Owner / 详见 |
|---|---|---|
| `Efficiency` | 生产域唯一效率类型，内部为 `i32` 千分位；`1.000` 表示基础 100%。 | [效率模型](EFFICIENCY_MODEL.md) |
| base / operator / global efficiency | 基础效率包含设施基准与非红脸（心情大于 0）进驻增量；干员技能效率与全局加成分别列出。 | [效率模型](EFFICIENCY_MODEL.md) |
| paper efficiency | 贸易基础效率、干员技能效率和全局加成之和。 | [效率模型](EFFICIENCY_MODEL.md#31-贸易) |
| unit output / multiplier | 社区确认的单位日产出，以及相对基准日产出的倍率。 | [效率模型](EFFICIENCY_MODEL.md#31-贸易) |
| total efficiency | 基础效率 + 干员技能效率 + 全局加成；不含特殊单位产出倍率或等效解释值。 | [效率模型](EFFICIENCY_MODEL.md) |
| final efficiency | 已从公开效率协议移除；内部实现仅作为排序变量名暂存。 | [效率模型](EFFICIENCY_MODEL.md)、[评分模型](SCORING_MODEL.md) |
| mechanic equivalent efficiency | 特殊机制的解释值；不参与第二次乘法，也不是排序真源。 | [效率模型](EFFICIENCY_MODEL.md#31-贸易) |
| weighted efficiency | 按班次分钟数对直接效率做的整数时长折算。 | [效率模型](EFFICIENCY_MODEL.md) |
| `DailyTotals` | 贸易、制造、发电分别保存的 24 小时加权汇总；不存在匿名跨域总计。 | [评分模型](SCORING_MODEL.md#2-排班汇总) |
| `DailyProduction` | 跨班次、跨房间按总周期归一到 24h 的五类最终产物平均值。 | [效率模型](EFFICIENCY_MODEL.md) |
| 会客室基础效率 | 会客室每名干员的 `干员技能效率 + 精英化基础加成 + 稀有度基础加成 + 非红脸进驻加成`；0 心情只使非红脸 `+5%` 失效。 | [效果原子设计](EFFECT_ATOM_DESIGN.md) |
| scoring policy | 对非生产或局部混合分量排序的具名规则，必须有稳定 ID 和 breakdown。 | [评分模型](SCORING_MODEL.md) |
| `ControlInjectRawSumV0` | 当前中枢普通候选的局部注入 policy；不是生产效率。 | [评分模型](SCORING_MODEL.md#3-非生产-heuristic) |
| `TradeManufactureWeightedEfficiencyV1` | 动态 producer 完整联合候选的具名贸易/制造加权 policy；当前默认 `1:1`，权重可显式调整，不改变分域效率字段。 | [评分模型](SCORING_MODEL.md#21-动态-producer-联合候选的具名加权-policy) |

## Bake 与验证

| 术语 | 短定义 | Owner / 详见 |
|---|---|---|
| Bake / 烘焙 | 预先生成单房候选 catalog，以减少兼容上下文中的实时组合求值。 | [质量总则](QUALITY_AND_AUDIT.md#6-bake-安全门禁)、[性能工程](PERFORMANCE_ENGINEERING.md) |
| baked catalog | `combo_table.bin`、operators、manifest 等本地产物；当前代码要求 schema v13。 | `bake.rs`、[性能工程](PERFORMANCE_ENGINEERING.md) |
| compatibility gate | 使用 Bake 前对 schema、输入指纹、候选身份和动态上下文进行的安全检查；不兼容走 live。 | [质量总则](QUALITY_AND_AUDIT.md#61-当前-schema-v13-已实现的门禁) |
| effect signature | 决定候选结算结果的上下文摘要；只有 owner 明确覆盖的签名才能安全缓存。 | [质量总则](QUALITY_AND_AUDIT.md#6-bake-安全门禁) |
| A+ Bake | 完整条件响应候选与安全 runtime join 的未来设计；当前不是已实现能力。 | [A+ TODO](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md) |
| fixture / 夹具 | 可重复构造输入的最小或标准数据，如 `data/fixtures/243/` 和 verify 房间。 | [Debug 指南](MAINTENANCE_MODE.md) |
| regression anchor | 锁定已确认数值或不变量的期望，如效率、单位产出、`rule_id` 或 required anchor。 | [质量总则](QUALITY_AND_AUDIT.md) |
| smoke test | 通过真实入口快速验证基础行为；不能代替定向不变量回归。 | [Debug 指南](MAINTENANCE_MODE.md#4-回归策略) |
| baseline | 性能、失败集合或账号画像比较使用的具名参考；必须记录具体文件和命令。 | [质量总则](QUALITY_AND_AUDIT.md#5-full-suite-与失败基线) |

## 容易混淆的八组词

1. **required admission / anchor ≠ `shift_bind`**：前者保证进编，后者只约束已入选成员怎样出勤。
2. **shortcut ≠ System / Rule**：前者结算实际组合，后者描述编排结构和候选路径。
3. **role ≠ 固定三人组**：role 保留核心或 fallback 边界，队友仍可由正式搜索选择。
4. **mechanic equivalent efficiency ≠ final efficiency**：前者解释机制，后者才用于生产排序与汇总。
5. **Team ≠ Shift**：Team 是共享节奏的成员组，Shift 是一个时刻的完整在岗状态。
6. **standalone roster ≠ `full_pool`**：前者是具名裁剪目录，后者是当前门禁后的完整普通候选池。
7. **`hard_constraint` ≠ heuristic / approximation**：只有 canonical 违规才是非法候选；启发式和近似必须保留或降低保证声明。
8. **current-reference ≠ canonical**：前者解释和导航，后者才拥有领域裁决权。
