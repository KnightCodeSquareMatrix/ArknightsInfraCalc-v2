# ADR 0014: 四赤金同场择优与自动轮换入口的实现路线

> 文档角色：decision
> 生命周期状态：proposed
> 当前真源：docs/模块/贸易求解.md；docs/调用链/plan请求.md；docs/排班/排班模式.md
> 摘要：评估十月排班剩余两项（四赤金完整候选比较、自动轮换入口）的实现难度，列出必须由用户裁决的事项，并给出按单一意图拆分的修改路线
> 代码范围：crates/infra-core/src/layout/orchestrate/；crates/infra-core/src/layout/assign.rs；crates/infra-core/src/schedule/；crates/infra-cli/src/commands/
> 核验基线：代码基线 `36c21d1`（2026-10-26）；本文未运行 build、CLI 或 invariant gate

> 日期：2026-10-26
> 决策状态：proposed
> 实现状态：两项均未实现；本 ADR 只记录调查结论、待裁决项和实现顺序，不修改源码、运行时数据或 fixture

## 背景与范围

[ADR 0012](0012-october-rotation-delivery-boundaries.md) 把四赤金完整 Trade 择优和自动轮换列为十月排班仍未完成的两项。[ADR 0013](0013-four-gold-trade-core-candidate.md) 已裁决四赤金是 `trade_core` 的一种专用候选，并把评分窗口、多贸易站聚合和平手规则留作 D4 未裁决项。

本文回答三个问题：这两项各自有多难；哪些点只有用户能定；在不违背 ADR 0012、0013 的前提下按什么顺序做。本文不重开 ADR 0013 的 D1 到 D3，不重开 2026-10-11 已裁决的自动轮换主体规则，也不接网站、MAA 或部署（用户 2026-10-24 裁决）。

## 核验状态

调查由只读 Agent 完成。其中关键事实已由主 Agent 回到源码核对，其余来自子代理笔记，实现前必须再核。

已由主 Agent 在 `36c21d1` 核对：

- `rules.rs` 的 `compile_rules` 按 `priority` 降序遍历（`rules.sort_by_key(|rule| std::cmp::Reverse(rule.priority))`）；被 `externally_skipped` 命中的规则只扩展 `skip_registry_ids_when_inactive` 后跳过，不占用 `exclusive_group`；排班首个可放置的 alternative 才占组。`pozyomka_durin`（1001）与 `trade_core`（1000）同属 `trade_core_selection`，所以先排成的赢，没有跨候选比较。
- `assign.rs` 的 `assign_shift_with_preclaimed_plan` 在 `GoldFlowCoreInfeasible` 时把 `"pozyomka_durin"` 插入跳过集合并重建 Plan，第二次失败才报错；`OptionalTradeRoleInfeasible` 重试上限为 4。
- `team_rotation.rs` 在 `ControlRotationRequiredAnchorsExceedCapacity` 时用 `fallback_skip` 整体重跑 ABC 构造。这是"第二次完整构造"的现成先例，但只覆盖 ABC。
- 菲亚 N/A/B 候选各自完整构造后比较：`value > winner_value + 1e-9` 才替换，平手保留 N。其度量是 `lmd + pure_gold + 1.25·battle_records`，不是贸易 `final_efficiency`。候选 B 只跳过规则 `"trade_core"`，没有跳过 `pozyomka_durin`。
- `base_rotation.rs` 对每个贸易房取 `trade_total_efficiency × 单位产出倍率` 并按站求和，`daily.trade` 是各班 `weighted_trade` 之和。`pipeline.rs` 的 `trade_manufacture_efficiency_totals` 仍累计纸面 `trade_total_efficiency`，不得复用于四赤金比较器。
- `resolve_base_core` 的参数 `_durin_dorm_planning` 带下划线前缀，未被使用；`apply_durin_dorm_planning` 只在 `main.rs` 的非排班路径调用。排班评分中的杜林人数来自实际 assignment。
- 仓库里没有 `auto_rotation` 标识符。
- 已删除的龙巫 profile（`22418b9` 之前的 `schedule_fiammetta_witch_12_12`）做法是：定位一间 3 级赤金贸易房，把巫恋、龙舌兰和一名 β 裁缝作为 `SystemAnchor`（`AnchorFillPolicy::TradeRole`）预占进该房，再走普通 Peak 构造，备班换另一名裁缝。即"人员先定死，其余走普通流水线"，不另写效率公式。
- 单房内"先定死一组人再与普通赢家比较"已有先例：`trade_fill.rs:790–827` 对鸿雪+图耶二人组取最佳第三人，与普通赢家比 `final_efficiency`，严格大于才替换，平手保留普通方案，且不占用核心 Plan。该先例只覆盖 `gold_production_lines == 2`。
- 杜林人数在评分中的作用点：`LayoutContext::durin_virtual_lines() = min(durin_in_base, 4)`；`trade/gold_flow.rs` 的测试直接设 `layout.durin_in_base = 4`；`search/trade.rs` 的 `project_trade_combo_layout` 已经把"未提交候选"投影进 layout 并增减杜林计数。也就是说"假定 4 名杜林在基建"可以在 layout 层表达，不必先把杜林落位。

子代理笔记中未复核、仅作线索的结论见各节标注（标记"待核"）。

难度与工程量是两个独立维度，下文分开标注：难度指机制与归属是否清楚、有无现成先例；工程量指要新增或改动的代码、数据、文档与证据的体量。

## 一、四赤金完整候选比较

难度：低到中。机制已清楚：核心三人固定，杜林按"基建内 4 名"假定算效率，之后才落位（延迟绑定），再与普通 `trade_core` 的单站结果比较；单房比较有 `trade_fill.rs:790–827` 先例。
工程量：S 到 M。要新增一个"假定杜林 4 名的核心房效率评估"、比较与平手规则、报告字段、4 金线 fixture 与证据。本节早先版本把"整轮重跑"当作比较器，那是为覆盖所有 profile 入口选的重方案，不是必需的；见 1.4。

### 1.1 现状缺口

1. 没有候选比较器：首个可行方案提交（见"核验状态"）。回退是单向的，只在四赤金失败后才重建 Plan。四赤金规则 priority 1001 高于 trade_core 1000，命中即压过其他核心；用户 2026-10-26 明确原始设计是"这几个 trade_core 都是正常选人比较竞争，没有优先级区别"，所以该优先级是当前实现的偏差，不是设计。
2. 被选中后的两处回退都不是择优：`GoldFlowCoreInfeasible`（核心房填不满）和中枢必需 anchor 超容量（仅 ABC 路径捕获）。ABC 的 ETA/心情重试不会跳过 `pozyomka_durin`。
3. 候选入口不止一处。ABC 的 `12/6/6`、`12/12/12` 与泛型 ABC 走 N/A/B 候选层；主备、`fiammetta-8844`、`abyssal-7575` 各自调用 Peak 构造，且传入空跳过集合。在 N/A/B 层加比较器覆盖不了后三者（待核：入口清单来自子代理笔记与 `grep &HashSet::new()` 的命中行）。
4. 菲亚候选 B 只跳过规则 `"trade_core"`，不跳过 `pozyomka_durin`，因此 N、A、B 三个菲亚候选内部都可能选中四赤金。比较器若放在候选层之内，会把构造次数乘以 2。

### 1.2 度量的读取点

- 单房：`TradeEfficiency::final_efficiency = total_efficiency × unit_output_multiplier`（ADR 0013 D3）。`mechanic_equivalent_efficiency` 只是解释值。
- 周期：`daily.trade` 是各班 `weighted_trade` 之和，每班值来自 `base_rotation.rs` 按站求和的 final，再按 `班时/周期` 的整数分钟比例缩放。`Efficiency` 是千分整数，平手是精确相等。
- 不能复用：`pipeline.rs` 的 `trade_manufacture_efficiency_totals`（纸面 total，无倍率）；`search/trade.rs` 的 `trade_efficiency_sort_key`（私有，且只比单房）。
- 因此"按站求和 + 周期加权"已是现有报告的事实口径，但 ADR 0013 D4 明确不允许把它当作已裁决规则，需要用户确认。

### 1.3 证据缺口

以下来自子代理笔记，待核：

- 已提交的 7 个 layout 赤金线均不超过 3 条，四赤金准入在已提交数据上不会触发；4 金线 layout 只存在于被 gitignore 的 `target/codex-runs/pozy-gold-unit-20261020/before/`。
- 38 个已提交 Box 加 `243/operbox_full_e2.json` 中，仅 `high-owned/b-101-424.json` 与 `243/operbox_full_e2.json` 同时满足三核心精二和至少 4 名杜林。
- 没有同 Box、同 layout 的"四赤金开 / 关"对照。历史样本中 `before/ordinary-243` 用默认 243 layout，`with-suggestions-243` 用 4 金线 layout，二者不同；其效率数字还早于贸易最终效率统一（`86c6027`），是纸面口径。**这些数字不得作为"四赤金更好或更差"的证据。**
- 没有覆盖四赤金准入或构造的 `#[test]`。`228/228` 批量中没有一例选中四赤金。
- 数据缺陷（2026-10-26 核对用户原始对话，已修复）：`durin_presence` 的候选顺序原为「杜林、特克诺、桃金娘、褐果、至简」，`Ordered` 截断取前四，所以会选特克诺、排除至简。用户 2026-09-28/29 的原始裁决是「杜林 > 桃金娘 > 褐果 > 至简 > 特克诺」，原始对话还专门对比过"当前顺序不对、方案要求如此"（`target/codex-runs/opencode-six-topics-20261019/readable/four-gold.md:93-103`，另见 :382、:497、:653、:908）。两个 alternative 的候选顺序已按裁决改正；用户 2026-10-26 明确不做回归测试。

### 1.4 设计（拟）

首选方案（轻量，沿用户路线）：在 Plan 提交四赤金之前做单站效率比较。用户 2026-10-26 裁决：比较窗口直接用单站，平手输出 N（普通方案）。

- 仅在 `pozyomka_durin` 的准入门槛已满足、且三核心均已精二时触发，其余输入零开销、行为不变。
- 核心身份（用户 2026-10-26 澄清）：鸿雪、绮良、图耶是固定成组的一个 trade_core，三人不拆；摆位按金单贸易站数走状态机：1 站为三人同站；2 站为「巫恋＋绮良＋龙舌兰 / 鸿雪＋图耶＋但书」；3 站为「巫恋＋绮良＋龙舌兰 / 图耶＋但书 / 鸿雪＋可露希尔（可用时）」。两站时但书由普通贸易补位进入；三站时三核心彼此不同房、各自与建议队友同房；建议队友缺失只减少该站收益，不影响核心成立。
- 代码差距：1 站同房形态对应的 `single_trade_gold_flow` 已在 `fb6dffd` 删除，当前只有恰好 2/3 站两个 alternative；实现固定三人 trade_core 需要按「恰好 1 间金单站」恢复该 alternative，并保留三核心同上同下绑定。
- 无优先级差别（用户 2026-10-26 裁决）：四赤金、可露希尔核心、巫恋组、但书核心都是正常的 trade_core 候选，按同一套选人与效率比较竞争；不存在"四赤金优先于其他核心"。当前 `pozyomka_durin` 的 priority 1001 对 `trade_core` 1000 是必须撤销的实现偏差。"优先分房"只约束四赤金候选自己的三人落位，不等于压过其他候选。
- 候选集合与选择责任（用户 2026-10-26 澄清）：四赤金固定三人组、可露希尔核心、巫恋组、但书核心都是 trade_core 候选，必须在同一个 trade_core 选择责任里按效率竞争。四赤金不得靠 priority 1001 直接压过其他核心；它只是候选之一。
- 评估口径：同一间赤金贸易站，按 4 条真实赤金线和「假定基建内有 4 名杜林」计算该站 `final_efficiency`；对手是其他 trade_core 候选按各自落位规则填房后的同一站 `final_efficiency`（可露希尔核心、巫恋组、但书核心），不是无约束的普通搜索。杜林假定通过 `TradeRoomInput.durin_virtual_lines = Some(4)` 或 `layout.durin_in_base = 4` 注入，不依赖已落位的杜林（`resolve_base` 的 `durin_dorm_planning` 参数当前未使用）。`Efficiency` 是千分整数，平手是精确相等，平手输出 N（普通核心）。
- 分房是固定状态机：核心与建议队友的房间归属由规则顺序、房间等级偏好和 room_group 决定，不参与效率比较；比较只发生在落位之后，读取每站最终效率。用户 2026-10-26 明确"分房的放法不需要管效率"。
- 读取点：Peak 贸易补位确实会把每站 `final_efficiency` 写入结果，但它发生在锚点落位之后、杜林已经占位，不能作为"比较先于绑定"的读取点；比较阶段必须自己按上述假定做一次单房试算（贸易 solver 可直接调用）。
- 评分：每个候选按自己的落位规则把赤金贸易站填好，逐站取单站 `final_efficiency`，把该候选下所有赤金贸易站的分数相加；分数最高者胜，平手输出 N（普通核心）。不再需要"每站都必须单独赢"的读法。
- 杜林延迟绑定：比较阶段只假定 4 名杜林，不落 anchor；比较通过后才让 `durin_presence` 的 4 个 anchor 真正占位。占位失败则整条 alternative 不成立。
- 比较不通过时整条 alternative 不成立，不写跳过集合、不占用 `exclusive_group`，`trade_core` 按自身顺序接管（已核对 rules.rs 418–437、641–643、662–670）。
- 触发点候选：`dynamic_gates_match` 已有 `ResourceAtLeast`、`TradeRoleFeasible` 一类"先 resolve 再决定"的门槛，说明规则编译期做 dry-run 有先例；比较应放在 `durin_presence` 角色放置之前，使其自然成为该角色的前置检查（待写入前确认）。
- 对已接受 ADR 0013 D3 的影响：D3 原文要求比较双方完整、合法的 Trade Plan，不能拿局部核心与完整结果比较。本条按用户 2026-10-26 裁决收窄为单站比较；ADR 0014 转 accepted 时须显式记录这一收窄。

备选方案（重）：在 `schedule_timed_rotation_request` 外层整轮重跑：先生成含四赤金的 A，若选中四赤金，用加跳过集合的克隆 options 重跑得 B，按完整周期 `daily.trade` 比较。一处覆盖主备、菲亚 8844、深海、ABC，且天然使用周期窗口；代价是最坏多一次完整构造，与菲亚 N/A/B 叠加最坏 6 次。仅当裁决 1 选"完整周期"时才需要它。

- 两种方案下，B（或普通对手）无解时四赤金直接胜出；四赤金本身失败仍走现有 `GoldFlowCoreInfeasible` 回退，不进入比较。
- 报告里记录两个候选的度量、窗口、平手规则与选择理由（ADR 0013"后果与验收边界"要求留存）。字段形态在写入单元里与序列化消费者一起确定。
- 保证等级：候选集合仅为 {含四赤金, 跳过四赤金} 两项，选择规则是 policy。它不证明全局最优，也不证明普通 `trade_core` 内部的择优。

### 1.5 风险

- 仅备选（重）方案：与菲亚 N/A/B 叠加时最坏 6 次完整构造，见裁决 3。首选（轻）方案不受影响。
- 与 r24 的下一写入单元（ABC `daily/warnings` 重复汇总，位于 `team_rotation.rs` 同区域）同文件相邻。保持单写者，顺序执行。
- `--assert-invariants` 只看最终 assignment，证明不了"选中者不劣于落选者"。拟加的门禁只能核对"报告记录的选择与最终 Plan 一致、且满足所裁决的比较规则"，这是自洽检查，不是最优性证明。
- `fa7ec47` 因制造搜索慢 3–8 倍删除过自动化整班择优。四赤金比较只在选中时多一次完整构造，量级不同，但写入后必须记录耗时。

## 二、自动轮换入口

难度：中。主体是"去掉部分功能的正常排班"：主班走既有 Peak 构造，备班走既有的互斥备班构造，禁令通过既有跳过与预过滤钩子施加。真正新的能力只有两点：覆盖所有候选入口的禁令（风险在漏入口），以及"按房间评估时长"。
工程量：L。新输出类型、新子命令、显式的允许 / 禁止分类表、专用终态断言、批量脚本透传，体量大但每项形态清楚。

### 2.1 已裁决（不重开）

2026-10-11 用户裁决（答复 `1A 2禁用 3可以使用 4允许 5A 6A 7正常输出效率计算 8包含`）与 2026-10-24 补充：

- 每站一主一替，工作成员互斥，主力优先；中枢同样分主备且不重叠。
- 禁止其他跨站协同，只保留宿舍挂件 1+1 与发电＋制造自动化；依赖建筑数量或等级的技能和中枢全局光环照常使用。中枢内部组合允许。
- 替补配不齐时保留主力、用普通散件补齐并标出缺员位。
- 菲亚固定不排，不排普通宿舍，不模拟错班。
- 每房评估到"组内第一人红脸"与 36 小时中较早者，该时长必须进入候选比较，不能选完名单再补算。
- 输出普通效率，不输出"参考日均产出"。
- 本轮不接网站、MAA 或部署；1+1 与 `support_combinations_enable` 只作实现兼容核对。

### 2.2 现状事实

- 没有 `auto_rotation` 标识符。`--rotation` 只接受 `named | main-backup@H1/H2 | abc@H1/H2/H3`；Worker `plan.compute` v7 的 `rotation` 是六个命名 profile 加通用 topology，`v1–v6` 为闭集。
- [排班模式.md](../排班/排班模式.md) 第 30 行要求自动轮换定义自己的状态，且不能靠给 `timed_rotation` 加布尔开关冒充。因此不扩展 `TimedRotationProfile`，也不复用 `TeamRotationReport`（定时语义的班次、休息队、每日加权、宿舍与周期心情都会一并带入）。
- 可复用的骨架（待核，来自子代理笔记）：`solve_owned_peak_assignment` 求主班；`solve_policy_disjoint_backup` 以主班已占姓名剔除后求备班；`fill_owner_office_seats` / `repair_owner_meeting_seats`。这些是 `team_rotation.rs` 私有函数，需要提升可见性或在同文件新增入口。
- 备班剔除口径用 `operator_names()`，包含宿舍成员，会把常驻宿舍挂件也从备班剔除，与"宿舍挂件可供主备共用"冲突（待核）。

### 2.3 难点

1. 禁令必须覆盖所有候选入口。Plan 规则、注册表体系、`complete_standalone_assignment`、`pick_trade_meta_then_plain` 的 role 搜索、`producer_fill.rs` 的宿舍 producer、`trade_fill.rs` 的鸿雪+图耶对比较、动态中枢 producer 都是独立入口（待核：入口清单来自子代理笔记）。只改 Plan 会漏。
2. 不存在可机读的"允许 / 禁止"分类。已核对 `orchestration_rules.json` 的 8 条规则 `tier` 全为 `cross_station`，无区分度；`base_systems.json` 的 `lungmen_manu_pair` 是中枢全局光环却被标为 `cross_station`；`DependencyScope` 把建筑数量类与特定干员在场类放在同一档（待核）。分类表需要新建，并按"覆盖对账失败即报错"的方式维护。
3. 每房评估时长。求解侧只有单个标量 `AssignBaseOptions.shift_hours`，贯穿贸易、制造、发电的填房与评分；`ShiftEta` 对整份 assignment 一次性计算，红脸时间取决于中枢减免，而中枢选人又取决于房间候选，存在环（待核）。现有解法是"中枢先于房间 + 终态复核"。
4. 门禁是整块的。`assert_final_rotation_invariants_with_mood` 以 `TeamRotationReport` 为输入，没有"主备互斥"的通用检查，需要新增针对自动轮换的最终态检查，并让 Python 批量脚本只透传新的 `--rotation` 标记，不重写机制。
5. 周期心情硬门禁未裁决（ADR 0012 第 47 行）。

### 2.4 设计（拟）

- 独立类型：新增 `AutoRotationReport`（每房主力、替补、效率、评估时长、自动化关联、宿舍挂件常驻说明、缺员位），不进入 `export/maa.rs`，不进入 `BoxProfile`。
- 入口：先挂 `layout auto-rotation` 子命令（只排班，不出 profile 与 MAA，与 `layout team-rotation` 同级）。Worker 方法与 schema 版本待用户需要网站时再定。
- 禁令实现为"准入层 + 最终态断言"两层：准入层在候选进入处过滤；断言层在最终 assignment 上检查，作为 hard constraint。分类表由 `data/` 中显式条目承载，每个体系、规则、alternative 都要标注，缺项即失败。
- 时长：按已裁决原话，时长进入候选比较；写入前先核实 `shift_hours` 的实际影响面（见裁决 8）。
- 保证等级：禁令为 hard constraint；选人为 heuristic；时长评估为 policy。不声称全局最优。

## 三、需要用户裁决的事项

每项给出建议默认值。默认值只是实现者建议，用户未确认前不得写成已裁决规则。

| # | 事项 | 影响 | 建议默认 |
|---|---|---|---|
| 1 | 四赤金比较窗口 | 决定读取点与聚合方式 | **已裁决（2026-10-26）：单站 `final_efficiency`，不折整轮周期** |
| 2 | 多贸易站聚合 | 每个候选按其落位规则填站；四赤金按 1/2/3 站形态填 | **已裁决（2026-10-26）：逐站取 `final_efficiency`，按候选占用的赤金贸易站求和后比较** |
| 3 | 是否与菲亚 N/A/B 叠加比较 | 单站比较发生在 Plan 编译期，与菲亚候选层无关 | 不再适用（仅备选重方案需要） |
| 4 | 平手 | `Efficiency` 是整数，平手是精确相等 | **已裁决（2026-10-26）：输出 N（普通方案）** |
| 5 | 是否同时要求制造和发电不变差 | 杜林占位可能影响制造 | 只比贸易；制造与发电差值作为诊断输出 |
| 6 | 2026-09-28 "只要使用，必定优先分房使用" 与 ADR 0013 "与普通完整候选竞争" | 前者是落位约束，后者是候选竞争 | **已澄清（2026-10-26）：所有 trade_core 正常选人竞争，无优先级差别；"优先分房"只约束四赤金候选自己的三人落位，不压过其他核心** |
| 7 | 自动轮换是否适用周期心情硬门禁 | 主备各一班时该检查近似恒真 | 不适用；计算入口不模拟恢复 |
| 8 | 自动轮换的红脸时长：进入候选比较，还是先保守选人、终态按每房时长评估 | 已裁决为"必须进入候选比较"。已核对：贸易排序与班时无关（按班时线性缩放）；`shift_hours` 的影响面看起来很窄，主要是制造心情差组合的 12 小时硬拒绝和发电站充能斜坡（制造效率斜坡按固定 20 小时均值，待核）。若属实，时长进入候选比较的改动面小于之前估计，也与中枢先于房间的环有关 | 按已裁决原话执行；仅当写入前核实改动面不小时，再请用户确认是否降级为终态评估 |
| 9 | 自动化中需要中枢森蚺的分支，以及莱茵宿舍 1+1 | 放行范围 | 只放行发电＋制造分支；莱茵 1+1 按"一挂件一受益者"放行，其余拒绝 |
| 10 | 是否提交 4 金线 layout fixture | 无 fixture 就没有四赤金回归证据 | 提交 243 与 342 两份 layout（仅数据） |

第 1、2、4 项已由用户 2026-10-26 裁决，不再是缺口。ADR 0013 D4 的比较窗口、聚合与平手三项据此落定；D3 的"必须比较完整 Plan"按本 ADR 收窄为单站比较，转 accepted 时一并记录。

## 四、实现路线

每个写入单元只有一个主要意图。每个源码单元开始前，先按 AGENTS.md 向用户说明输入、状态改变顺序、输出、保持不变的行为、允许变化和验证场景。与 r24 的 ABC `daily/warnings` 汇总清理共用 `team_rotation.rs`，所以排在其后，并保持单写者。

### 四赤金

1. **U1 fixture（纯数据）**：提交 243 与 342 的 4 金线 layout，并记录所用 Box。不改行为。
2. **U2 跳过钩子**：`AssignBaseOptions` 增加体系级跳过集合并并入 `assign_shift_with_preclaimed_plan`。默认空，对现有输出无影响。
3. **U3 trade_core 候选竞争**：把四赤金（含恢复的 1 站同房形态）、可露希尔核心、巫恋组、但书核心放进同一个 trade_core 候选比较层；各自按固定落位规则填站，逐站取 `final_efficiency` 求和选最高，平手输出 N；撤销四赤金 priority 1001 的压过行为。四赤金候选按假定 4 名杜林评估，胜出后才绑定杜林。依赖裁决 1、2、4、5、6。
4. **U4 证据**：同 Box、同 layout 的开 / 关 A/B；在 `high-owned/b-101-424.json` 与 `243/operbox_full_e2.json` 上留存双方完整最终 assignment、度量和选择理由。
5. **U5 门禁**：在 Rust 最终 assignment 上加自洽检查（报告所记选择与最终 Plan 一致，且符合所裁决的比较规则），Python 批量对代表与 high-owned 集合只调用该 gate。

### 自动轮换

1. **U6 文档**：更新 `docs/排班/排班模式.md` 第 17、30 行与 `docs/指南/GLOSSARY.md` 第 110 行（"按组合耗尽触发" 与已裁决的 "首人红脸或 36 小时" 不一致），新建自动轮换 canonical 语义页。
2. **U7 分类表**：显式列出每个体系、规则、alternative、角色与 producer 的"允许 / 禁止 / 委托"，缺项 fail-closed。
3. **U8 准入接线**：把 U7 接到所有候选入口，并加最终态断言。
4. **U9 二态求解与报告类型**：主备互斥、缺员补位与标注、`AutoRotationReport`、`layout auto-rotation` 子命令。
5. **U10 时长评估**：按裁决 8 实现。
6. **U11 门禁与批量**：新增自动轮换专用断言，批量脚本透传新标记。

### 文档传播

每个源码单元同步复核 `docs/模块/贸易求解.md`、`docs/调用链/plan请求.md`、`docs/接口/CLI与Worker.md` 与 route registry，并运行：

```text
python3 scripts/codex/generate_agent_index.py --check
python3 scripts/codex/docs_inventory.py --check
python3 scripts/codex/check_repository_facts.py
```

## 五、验证要求

- 全部使用 release 构建：`cargo build --release -p infra-cli`，`target/release/infra-cli`；修改 Rust 后 `cargo fmt --all`。
- 不改变现有输入输出的单元（U2）：证明现有 `38 × 6` 批量输出不变。
- U3 到 U5：使用 `arknights-invariant-gate` 与 `arknights-evidence`，并经 Sol 复审至 `PASS`。
- 注意：`.agents/skills/arknights-invariant-gate/SKILL.md` 写"不要新增或运行源码测试"，AGENTS.md 又给出 `cargo test --release`。二者对 Rust 源码测试是否允许不一致（待核），写入前由用户或文档所有者澄清；澄清前以 gate 与 CLI 证据为准。

## 六、不在本 ADR 范围

- 全局最优、跨 layout 的完整方案比较、普通 `trade_core` 内部择优。
- 网站、MAA、Worker schema 新版本与部署。
- Mower 动态换班、全基建连续心情最优化。
- 菲亚、普通宿舍与错班模拟（自动轮换已明确不做）。
- 已有文档矛盾的统一整理（`docs/TODO/详情/排班轮换/排班统一内核重构方案.md` 第 714 行与 781、869 行；`docs/调用链/plan请求.md` 第 60 行与 62 行）。它们只在此处登记，不在本 ADR 中修改。

## 后果

- 本 ADR 为 `proposed`。用户确认第三节后，才能转为 `accepted` 并启动 U3；U1、U2、U6、U7 不依赖第 1–5 项裁决。
- 四赤金的完成标准以 ADR 0013"后果与验收边界"为准，本 ADR 不放宽。
- 本 ADR 不改变 ADR 0012 的状态判定。

## 依据

- [ADR 0012](0012-october-rotation-delivery-boundaries.md) 的第 45、47 行与关闭边界 2、4。
- [ADR 0013](0013-four-gold-trade-core-candidate.md) 的 D3、D4。
- [贸易求解](../模块/贸易求解.md) 与 [plan 请求调用链](../调用链/plan请求.md) 中关于首个可行候选与缺失比较器的记录。
- [排班模式](../排班/排班模式.md)、[排班统一内核重构方案](../TODO/详情/排班轮换/排班统一内核重构方案.md) 的 r24 检查点。
- 自动轮换用户原话与裁决：`target/codex-runs/opencode-six-topics-20261019/readable/`（本地证据，不入库）。
