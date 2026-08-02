# 项目总览：从技能语义到 MAA 排班

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/EFFECT_ATOM_DESIGN.md；docs/BASE_ASSIGNMENT.md；docs/ORCHESTRATION_LAYER.md；docs/排班模式.md
> 摘要：提供项目当前范围和能力总览

> 实现快照：Current
> 读者：策略作者、开发者、维护者、AI 协作者
> 本文负责：解释当前端到端运行模型；不新增领域公式
> 业务真源：用户当前裁决与对应领域 Markdown

本文面向想真正理解 ArknightsInfraCalc v2 的策略作者、开发者和 AI 协作者。它解释当前实现如何把一份练度盒和一张基建蓝图，变成可结算的单班编制、具名 timed rotation 和 MAA JSON。

如果只想先知道项目有什么用，读 [README](../README.md)；遇到陌生名词时查 [GLOSSARY](GLOSSARY.md)。文件级代码地图见 [PROJECT_MAP](PROJECT_MAP.md)。

## 1. 它实际上在求解什么

输入不是一串孤立的技能百分比，而是三个互相约束的对象：

- `OperBox`：玩家拥有哪些干员、精英化和等级如何；
- `BaseBlueprint`：有哪些房间、等级、容量和配方；
- 领域语义：技能如何结算、哪些成员是硬核心、哪些关系要求同房或同班、哪些只是自然效率关系。

输出也不只是“最高效率三人组”：

- 每个生产房间的直接效率与机制解释；
- 每个 `room_id` 的实际进驻成员；
- 中枢、生产设施和跨设施 producer/consumer 的一致布局；
- 具名 timed profile 中各 Team/Shift 的上岗、休息与加权效率；
- 可序列化的 core `BaseAssignment`、账号画像 JSON 和 MAA 排班 JSON。

核心困难来自组合耦合：同一人只能占一个岗位；前一房的成员可能改变后一房的 workforce、标签计数或全局资源；某些体系缺一人就必须关闭，另一些组合则应该允许搜索自然降级。引擎的目标不是抹平这些差异，而是让每类规则只在正确的责任层出现一次。

## 2. 真源与运行时载体

业务语义的优先级是：

1. 当前对话中用户明确确认或纠正的口径；
2. 对应领域唯一 canonical Markdown 中的不变量与预期行为；
3. 当前代码和生成 help 所证明的实现事实；
4. JSON、CSV、fixture、Bake、测试和旧输出等实现载体或核对材料。

低层材料可以帮助定位实现，但不能反过来推翻上层语义。若两个领域 canonical Markdown 互相冲突，实施者必须先让用户裁决，而不是依据现有输出猜一个答案。

主要运行时文件各有单一职责：

| 文件 | 职责 |
|------|------|
| `data/operator_instances.json` | 干员在 `tier_0` / `tier_up` 的设施技能归属和 `buff_id` |
| `data/skill_table.json` | `buff_id` 对应的声明式 `EffectAtom` |
| `data/trade_shortcuts.json` | L3 组合规则、社区单位产出与解释锚点 |
| `data/trade_segments.json` | producer 前提、consumer 组合和贸易 role pick steps |
| `data/orchestration_rules.json` | 当前声明式 Rule、alternative、role、relation 和 admission catalog |
| `data/base_systems.json` | legacy compatibility registry 与中枢制造注入器 |
| `data/producer_rules.json` | 动态 producer 的响应依赖、资源读取和 reverse closure 声明 |
| `data/REGRESSION_CASES.csv` | 最终效率、机制解释与规则 ID 的回归锚点 |
| `data/UNIT_OUTPUT_ANCHORS.csv` | 社区单位产出和赤金消耗锚点 |

结构不变量之一是 `skill_table.id == buff_id`。L1 解释器只接收 `buff_id` 和已编译 atom；干员中文名归属只在 instance 层解决。

## 3. 从原文机制到可执行 EffectAtom

一个普通技能被拆为若干平坦 atom：

```text
EffectAtom = Selector + Action + optional Condition + Phase + Scope
```

- `Selector` 决定从哪里读数，例如订单上限、宿舍人数、贸易站数量或带某标签的人数；
- `Action` 决定做什么，例如增加效率、改变订单上限、写入状态池或注入制造效率；
- `Condition` 决定何时触发，例如搭档同房、某人在基建内或心情达到阈值；
- `Phase` 决定执行先后，避免“先清零还是先加成”依赖代码偶然顺序；
- `Scope::Room` 只影响当前房间，`Scope::Global` 由跨设施编排器收集。

`atoms: []` 不等于“忘了建模”。它可以表示该技能已经登记，但执行权被明确委托给 L2/L3 领域引擎。

完整词汇和现有 selector/action/condition 见 [EFFECT_ATOM_DESIGN](EFFECT_ATOM_DESIGN.md)。

## 4. 为什么贸易求解分成 L1 / L2 / L3

贸易是当前机制最完整的领域，单房主路径是：

```text
TradeRoomInput
  → L1 interpreter：按 Phase 执行 EffectAtom
  → L2 gold_flow：赤金虚拟线和链式状态
  → L3 shortcut 或 L2 order_mechanic
  → unit_output
  → TradeResult / final_efficiency
```

### L1：通用、声明式、与干员名解耦

L1 适合固定效率、同房人数、标签条件、订单上限和状态写入等可以组合的机制。新增一个已有词汇能表达的技能，通常只需要改数据。

### L2：领域机制，不伪装成通用 atom

赤金闭环、订单分布、违约、裁缝、特别订单和单位产出不是简单的“加 X%”。它们需要领域状态机和分布模型，因此进入 `gold_flow`、`order_mechanic`、`unit_output`。

### L3：对实际组合结算，不替编排选人

但书、巫恋、可露希尔、黑键、推王和企鹅等部分组合存在社区确认的单位产出或固定最优档。L3 根据实际同房成员和 producer 前提匹配 `rule_id`，再给出单位产出倍率。

shortcut 的责任到此为止：它不能因为某个组合很强，就在编排层提前强塞成员。候选是否入选仍由 hard constraint、role policy 和效率搜索决定。

制造当前使用 L1 + 单房搜索，没有照搬贸易的 L2/L3。发电也有独立解释器与搜索。各设施共享数值类型和布局上下文，不共享不适用的领域假设。

## 5. 直接效率、机制解释与命名 policy

生产域统一使用 `Efficiency`：内部是千分整数，输出是三位小数直接效率。

```text
1.000 = 基础 100%
1.550 = 基础效率的 155%
```

贸易最终效率由纸面效率和单位产出倍率共同决定；制造和发电各有自己的直接效率公式。`mechanic_equivalent_efficiency` 解释社区机制，但不会再乘一次。

排班始终分开保存：

- `trade_efficiency`；
- `manufacture_efficiency`；
- `power_efficiency`。

项目没有匿名跨域总分。确实需要局部排序时，必须使用有名字、有分解的 policy。例如中枢普通补位当前使用 `ControlInjectRawSumV0`：

```text
trade_inject + manu_gold + manu_battle_record
```

它只是中枢候选的局部 heuristic，不冒充生产效率，也不进入每日生产总计。数值契约见 [EFFICIENCY_MODEL](EFFICIENCY_MODEL.md) 和 [SCORING_MODEL](SCORING_MODEL.md)。

## 6. Rule、兼容 System、Plan 与自然搜索怎样共存

单班编排不是把所有高配队写死，而是把确定性和自由度分开：

```text
compile Rule alternatives + merge legacy registry
  → build AssignmentPlan
  → execute fixed / bond / anchor slots
  → 按设施 fill 剩余位置
  → resolve 完整布局
```

### 硬约束负责排除非法状态

典型硬规则包括：

- 可用角色足够时，中枢必须补满 5 人；
- 激活的迷迭香感知体系必须同时包含迷迭香 E2 和黑键 E2；
- 自动龙巫站必须包含巫恋、龙舌兰和裁缝 α/β；
- required anchor 不能被后续 fill 替换；
- 明确禁止同房的机制在搜索和 solver 入口都拒绝；
- 明确要求同上同下的成员必须生成 `shift_bind`。

这些规则描述“什么状态不合法”，不应该被 `top_k`、tag、priority 或 shortcut 间接碰运气保证。

### 自然搜索负责在合法空间里选高效队友

典型自然自由度包括：

- 但书作为贸易核心时，队友按当前上下文的 `final_efficiency` 选择；
- 可露希尔缺黑键时仍保留核心，再搜索其他合法队友；
- 叙拉古消费者可以不上、单走、跨站或自然同站，不把同站写成激活前提；
- 普通制造候选不由贸易工具人表裁剪。

`role` 处于二者之间：它表达“优先考虑哪个核心或 fallback 链”，但 role 内部仍然调用正常单房 solver。详见 [ORCHESTRATION_LAYER](ORCHESTRATION_LAYER.md) 和 [BASE_ASSIGNMENT](BASE_ASSIGNMENT.md)。

## 7. 单班全基建生命周期

当前主路径按以下顺序工作：

1. 加载蓝图、operbox、instance 和技能表；
2. `build_plan` 编译声明式 Rule、汇合 legacy registry，并解析 fixed/bond/required anchor；
3. `execute_plan` 把已解析 placement 写入初始编制和全局 `used`；
4. 按练度解析实际 `buff_id`，建立并标注中枢、贸易、制造、发电候选池；
5. 中枢、producer、发电、贸易和制造按 pipeline 的具名顺序 fill/search，命中后立即提交；
6. 依赖已提交 workforce 的后续阶段反复 `resolve_base`，重建当前完整上下文；
7. 全部落位后刷新房间效率快照，返回 `BaseAssignment`。

`resolve_base` 本身会构造上下文和各房间的 resolved input：

```text
WorkforceIndex
  → 发电站求解
  → 中枢求解
  → 办公室求解
  → 收集并执行 scope=Global atom
  → 全局资源转化
  → 构造各贸易 / 制造 / 发电房输入快照
```

它不替代各设施 solver。贸易、制造和发电的候选搜索或最终刷新，会继续把这些 resolved input 交给各自求解器。

`used` 是跨设施唯一占用集合。搜索候选若与它相交，就不能提交；不能先让两个设施各自求最优，再在输出层删除重复名字。

当前单班分配是领域约束下的分阶段、逐房搜索。组合内部可以并行，房间之间若存在 workforce 依赖则顺序提交。它避免了房间笛卡尔积，但也因此不宣称全基建全局最优。

## 8. 跨设施资源与注入

跨设施机制有两种主要形态：

1. `GlobalResourcePool` 中的可共享资源，例如感知信息、人间烟火、木天蓼、魔物料理和虚拟发电；
2. `GlobalInjectManifest` 中由中枢产生的贸易 / 制造注入和 producer 前提。

`cross_facility` 会扫描所有 `scope=Global` atom，按 Phase 执行状态生产和转化。消费房读取最终快照，因此办公室、宿舍、中枢和生产设施不需要互相按干员名调用。

跨设施关系不自动等于固定体系：producer 与 consumer 是否必须同时进编、是否要求同房、是否要求同班，都要由领域 Markdown 分别说明。

当前动态 producer 已由统一 catalog、presence 集合比较和 winner dependency 处理；目标房仍按具名有序填房 policy 提交，不保证跨房 exact joint。未来的完整候选列、响应签名索引 Join 与跨房精确连接记录在 [DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)，不得把该 TODO 当作当前能力。

## 9. Timed profiles 与默认 αβγ ABC

`schedule_timed_rotation` 在默认 ABC、二班 `12/12`、菲亚 `8/8/4/4` 和深海 `7/5/7/5` 四个具名 profile 中分派。默认 ABC 先生成当前启发式流程选出的 `peak` 主力编制和对应 `AssignmentPlan`，再把生产设施切成 H1/H2 两半：

```text
α = peak 的 H1
β = peak 的 H2
γ = 两个半区的替补队

S1 12h：α + β 上岗，γ 休息
S2  6h：β + γ 上岗，α 休息
S3  6h：γ + α 上岗，β 休息
```

中枢不简单复制 peak 的五人，而是按活跃两队重新补满；休息队成员不得偷跑进当班中枢。宿舍保持共享，办公室按 αβγ cohort 轮换；绑定成员随消费方体系同上同下，未绑定成员也不跨班永久共享。

`shift_bind` 负责跨设施成员的同上同下和上二休一。它从计划或 peak 实际落位派生，只约束已经合法入选的成员。当前最终 runtime validator 会显式验证：

- 每班中枢恰好 5 人；
- 显式 bind 的 presence 向量一致；
- producer 只在正确设施统计，consumer 只在正确消费设施统计；
- bind 在三班中满足规定的上岗 / 休息次数。

生产设施容量、满编和同班人员互斥主要由候选构造、全局 `used` 与分层回归保证；它们仍属于端到端验收项，但当前不是全部集中在同一个 final validator 中。

当前报告还会计算 peak 主力的心情 ETA 锚点，并支持一次轻量菲亚梅塔主力回岗覆盖；默认 ABC 的 `12h + 6h + 6h` 不会根据 ETA 自动改写。显式 `--rotation` 另支持二班 `12/12`、菲亚 `8/8/4/4` 和深海 `7/5/7/5`，均以具名 profile 和硬失败语义进入。完整契约见 [SCHEDULE_ROTATION](SCHEDULE_ROTATION.md)。

## 10. CLI、账号画像与 MAA

`infra-cli` 是薄外壳：它加载文件、调用 `infra-core`、格式化输出，不承载机制公式。

推荐入口：

```bash
cargo run -q -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/overview-profile.json \
  --maa-out out/overview-maa.json
```

`plan` 通过共享 compute owner 先运行一次 timed rotation probe（默认 ABC），再按请求从该结果派生账号画像和 MAA。只需要轮换时使用 `layout team-rotation`；只做单班探测时使用 `layout test`；指定编制复算时使用 `layout eval`。

MAA 导出消费最终各班 assignment，而不是重新解释效率。`plan` 只生成一次用户 rotation；profile JSON、最终 CLI 排班文本和 MAA 都消费该同一结果。profile 仍只保存分析与指标快照，不包含完整 MAA assignment；其中 baseline 对照会独立评估固定基准，但不会重跑用户 rotation。

## 11. 正确性优先的性能设计

当前性能策略遵守一个原则：缓存和缩池可以拒绝工作，但不能悄悄改变答案。

- 工具人池用于贸易等明确适用的领域；普通排班制造使用全部合法普通候选；
- 单房 `C(n,k)` 组合求值用 Rayon 并行；
- 当前跨房依赖通过顺序 commit + `resolve`，保证后房能看到前房已提交状态；前房选型仍看不到后房最终成员，双向联合反馈属于未来 A+ 的已知改造边界；
- Bake 保存 schema v12 单房候选索引、整数效率和首批贸易 room-local 机制签名，并校验完整贸易 row universe；
- Bake 使用前检查数据指纹、CLI 生成器、房间签名、练度和布局兼容性；
- 动态候选投影、非基准布局或旧 schema 等不安全情况会走实时搜索。

因此“命中 Bake”是优化，“live solver”才是语义后备。未来 A+ Bake 计划希望扩大可安全复用的候选全集，但目前没有实现，见前述 TODO。

## 12. 为什么这套结构适合 AI 维护

AI 写代码最危险的地方不是语法，而是把一个样例的偶然输出误当成领域规则。本仓库用几道边界降低这种风险：

- [AGENTS.md](../AGENTS.md) 负责任务分类、真源优先级和项目硬门禁；
- [MAINTENANCE_MODE.md](MAINTENANCE_MODE.md) 要求先复现、再缩小到 CLI / layout / search / solver / mechanism / data 中的一层；
- [SYSTEM_AUDIT_WORKFLOW.md](SYSTEM_AUDIT_WORKFLOW.md) 要求体系修改前写出不变量、违规位置、单一责任边界和删除清单；
- 数据、解释器、领域引擎、编排、排班和导出有明确依赖方向；
- 回归断言尽量钉不变量，而不只是钉一张最终快照；
- 每次真实 CLI 和测试验证都应留下完整日志与生成 JSON 的可点击路径。

这让 AI 可以承担大量机械审计、候选枚举和回归补齐，同时仍由 Markdown 和用户裁决控制业务语义。

## 13. 当前边界

本项目当前不负责：

- 宿管自动分配和完整恢复周期；
- 根据心情 ETA 自动决定所有班次长度；
- 全基建连续时间最优化；
- 跨贸易、制造、发电的匿名综合收益最大化；
- 对所有房间做无限制联合穷举；
- 用历史 TODO 自动扩张当前维护范围。

当任务已由 [AGENTS.md](../AGENTS.md) 分类为 maintenance 时，目标是复现具体问题、修正真实责任边界、补不变量回归并保持已有口径稳定；这不是项目的默认生命周期阶段。

## 继续阅读

| 主题 | 文档 |
|------|------|
| 文档与任务路由 | [INDEX.md](INDEX.md) |
| 术语 | [GLOSSARY.md](GLOSSARY.md) |
| 一次真实 `plan` 的代码调用链 | [ARCHITECTURE_TOUR.md](ARCHITECTURE_TOUR.md) |
| 可重复的 243 全精二案例 | [EXAMPLES/243_FULL_E2.md](EXAMPLES/243_FULL_E2.md) |
| 模块地图 | [PROJECT_MAP.md](PROJECT_MAP.md) |
| 质量证据与性能边界 | [QUALITY_AND_AUDIT.md](QUALITY_AND_AUDIT.md)、[PERFORMANCE_ENGINEERING.md](PERFORMANCE_ENGINEERING.md) |
| 效率与评分 | [EFFICIENCY_MODEL.md](EFFICIENCY_MODEL.md)、[SCORING_MODEL.md](SCORING_MODEL.md) |
| 单班编制 | [BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md) |
| 编排层 | [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md) |
| Timed rotation | [排班模式.md](排班模式.md)、[SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md) |
| CLI 与前端 | [INFRA_CLI.md](INFRA_CLI.md)、[FRONTEND_CLI.md](FRONTEND_CLI.md) |
| 当前维护流程 | [MAINTENANCE_MODE.md](MAINTENANCE_MODE.md) |
