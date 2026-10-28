# ADR 0014: 四赤金同场择优与自动轮换建议模式的实现路线

> 文档角色：decision
> 生命周期状态：proposed
> 当前真源：docs/模块/贸易求解.md；docs/调用链/plan请求.md；docs/排班模式.md
> 摘要：评估十月排班剩余两项（四赤金完整候选比较、自动轮换建议模式）的实现难度，列出必须由用户裁决的事项，并给出按单一意图拆分的修改路线
> 代码范围：crates/infra-core/src/layout/orchestrate/；crates/infra-core/src/layout/assign.rs；crates/infra-core/src/schedule/；crates/infra-cli/src/commands/
> 核验基线：代码基线 `36c21d1`（2026-10-26）；2026-10-28 复核基线 `db0755b`；本文未运行 build、CLI 或 invariant gate

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

**历史说明（2026-10-28）**：本节已被新设计 `docs/TODO/四赤金与贸易站核心枚举设计.md` 取代（另一工作树同期撰写，状态 `proposal`），保留原文作为调查与决策历史。新设计把四赤金的选型改为 Plan 编译期的 `trade_core` 候选枚举与联合选择，取代本节 1.4 的方案；已有工作树的比较包装不移植。四赤金的实现路线以该文档为准，本节不再作为实现依据。补充裁决（2026-10-28）：用户答“四赤金和中枢无关”（`0b5face2`，2026-10-28 结构化问答）；中枢必需锚点超容量时跳过四赤金（`team_rotation.rs:7766-7789` 的 `ControlRotationRequiredAnchorsExceedCapacity` 分支）属于错误归因的补偿，四赤金设计的单元 1b 删除，不并入“候选不可行”。

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

## 二、自动轮换建议模式

难度：中。主体是"去掉部分功能的正常排班"：主班走既有 Peak 构造，备班走既有的互斥备班构造，禁令通过既有跳过与预过滤钩子施加。真正新的能力只有两点：覆盖所有候选入口的禁令（风险在漏入口），以及"按房间评估时长"。
工程量：L。新输出类型、新子命令、显式的允许 / 禁止分类表、专用终态断言、批量脚本透传，体量大但每项形态清楚。

### 2.1 已裁决（不重开）

2026-10-11 用户裁决（答复 `1A 2禁用 3可以使用 4允许 5A 6A 7正常输出效率计算 8包含`；问题与选项原文见 `target/codex-runs/opencode-six-topics-20261019/readable/auto-rotation.md:170-239`，用户消息见同目录 `auto-rotation.users.md` #0012、#0014）与 2026-10-24 补充（会话 `1d9851f1` L3：“挂件 1+1 与总开关只作为实现兼容核对”“不擅自接网站、MAA或部署”）：

- 每站一主一替，工作成员互斥，主力优先；中枢同样分主备且不重叠（“每站一主一替”“工作成员互斥”是用户原话；“中枢分主备”由“为每一个站设计完全不同的轮换组…1主1替补”推断，后由 2026-10-28 结构化问答“分主备、不重叠 (Recommended)”确认为用户裁决；见 canonical 页 3.1）。
- 禁止其他跨站协同，只保留宿舍挂件 1+1 与发电＋制造自动化；依赖建筑数量或等级的技能和中枢全局光环照常使用。中枢内部组合允许（“依赖建筑数量或等级…”来自第 2 题所选 A 的选项说明，选项文字由 agent 撰写；见 canonical 页 3.2 的出处标注）。
- 替补配不齐时保留主力、用普通散件补齐并标出缺员位。
- 菲亚固定不排，不排普通宿舍，不模拟错班。
- 每房评估到"组内第一人红脸"与 36 小时中较早者（用户 2026-10-11 第 1 题答 1A）；该时长必须进入候选比较、不能选完名单再补算是 agent 对该口径的实现解读。
- 输出普通效率，不输出"参考日均产出"。
- 本轮不接网站、MAA 或部署；1+1 与 `support_combinations_enable` 只作实现兼容核对。
- **自动轮换中禁用四赤金（2026-10-28 裁决）**：结构化问答问“自动轮换模式里，四赤金能不能用？10-11 的裁决是‘禁止其他跨站协同，只保留宿舍挂件 1+1 与发电＋制造自动化’。四赤金依赖全基建杜林人数和跨贸易房的虚拟赤金线。”，用户答“禁用”（2026-10-28 结构化问答；原始会话 `0b5face2` L963；本轮原话汇编 G 段）。U7 分类表与 U8 接线都必须把 `pozyomka_durin` 及其角色/alternative 标为禁止；不得以“四赤金是 trade_core 候选”为由放行。

### 2.2 现状事实

- 没有 `auto_rotation` 标识符（已核对 `contracts/`、`crates/`、`data/` 无命中，只有文档提到）。`--rotation` 只接受 `named | main-backup@H1/H2 | abc@H1/H2/H3`（`rotation_types.rs:392-458`）；Worker `plan.compute` v7 的 `rotation` 是六个命名 profile 加通用 topology（`contracts/plan-compute-v7.schema.json:45-90`），`v1–v6` 为闭集（`docs/接口/CLI与Worker.md:54`）。
- [排班模式.md](../排班模式.md) 第 30 行要求自动轮换定义自己的状态，且不能靠给 `timed_rotation` 加布尔开关冒充。因此不扩展 `TimedRotationProfile`（枚举定义见 `rotation_types.rs:195-225`），也不复用 `TeamRotationReport`（`rotation_types.rs:573-593`：profile、teams、shifts、按周期归一的 `daily`、`peak_mood_eta` 等定时语义会一并带入）。
- 可复用的骨架（已核对，`db0755b`）：`solve_owned_peak_assignment`（`team_rotation.rs:9242`）是单班 Peak 求解骨架，主班与备班共用，并非只求主班；`solve_policy_disjoint_backup`（`:9323`）用 `main.operator_names()` 从 Box 剔除后求备班（`:9334-9335`）；`fill_owner_office_seats`（`:9588`）与 `repair_owner_meeting_seats`（`:9642`）只补办公室/会客室空位，不参与主备选人。四者都是 `team_rotation.rs` 私有函数（`fn`，无 `pub`），需要提升可见性或在同文件新增入口。
- 备班剔除口径用 `operator_names()`，已核对包含宿舍成员：`BaseAssignment::operator_names()` 遍历全部房间（`layout/assignment.rs:470-478`），紧随的注释把占用口径定义为“任意房间（含宿舍）成员加独立的训练副手”（`:480-486`）；排除宿舍的岗位口径是另一个方法 `work_position_operator_names`（`:486-511`）。主班 Peak 会调用 `assign_dorm_producers` 填宿舍 producer（`pipeline.rs:1331`、`producer_fill.rs:80`），所以备班会把主班的宿舍挂件/producer 一起排除，与用户裁决“宿舍挂件 1+1 可主备共用”（2026-10-11 第 3 题“3可以使用”）冲突。这是实现事实，不是假设。

### 2.3 难点

1. 禁令必须覆盖所有候选入口（已核对入口清单）：
   - Plan 规则编译：`rules.rs:371`；注册表体系选择：`select.rs:189`（`build_plan_with_runtime_and_disabled_packs` → `compile_plan_once`），条目在 `data/base_systems.json`。
   - `complete_standalone_assignment`（`assign.rs:288`）在 `assign_shift_with_plan`（`:284`）之后补办公室与空缺；轮换骨架走 `assign_shift_with_plan_skip`（`team_rotation.rs:9252`，定义在 `assign.rs:458`），不经过它，但若自动轮换另走 standalone 入口，仍需单独覆盖。
   - 补位梯子 `pick_trade_meta_then_plain`（`trade_fill.rs:496`）及其内部 role 排序（`role_pick.rs:278-289`）。
   - 宿舍 producer `assign_dorm_producers`（`producer_fill.rs:80`，由 `pipeline.rs:1331` 调用）。
   - `trade_fill.rs:790-827` 的鸿雪+图耶二人组与普通赢家比较（`gold_production_lines == 2`）。
   - 动态中枢 producer `dynamic_producer_candidates`（`pipeline.rs:1466`）。
   只改 Plan 会漏。阶段 1（四赤金 trade_core 枚举重构）会删除上述补位梯子中的但书/可露希尔/巫恋步骤与鸿雪+图耶同房特判；U8 在阶段 1 合入后接线，不给这些路径接线（见 U8 注）。
2. 不存在可机读的"允许 / 禁止"分类。已核对：`data/orchestration_rules.json` 的 8 条规则 `tier` 全为 `cross_station`（`mujica_fallback`、`pozyomka_durin`、`trade_core`、`rosemary_perception`、`automation_group`、`pinus_sylvestris`、`human_fireworks_perception`、`rhine_receivers`），无区分度；`data/base_systems.json:103-105` 的 `lungmen_manu_pair` 是中枢全局光环（“龙门组中枢：斩业星熊+诗怀雅（全制造+3%）”）却标为 `cross_station`；`DependencyScope`（`response_dependency.rs:16-23`）确实把建筑数量类和特定干员在场类放在同一档，例如 `TradeStationCount` 与 `OperatorInTrade` 都是 `CrossFacility`（`:1220`、`:1269`），`EliteFacilityCount` 与 `OperatorInBase` 都是 `GlobalLayout`（`:1212`、`:1266`）。分类表需要新建，并按"覆盖对账失败即报错"的方式维护。
3. 每房评估时长。求解侧只有单个标量 `AssignBaseOptions.shift_hours`（`assign.rs:51`），传入贸易、制造、发电求解（`trade_fill.rs:434`、`manufacture_fill.rs:169/460`、`power_fill.rs:108/193`）。已核对：贸易效率不随班时变化，`shift_hours` 只影响产量/日换算（`trade/solver.rs:53-56、231、246`）；制造在房间有心情差效果时按实际 `shift_hours` 做分段平均（`manufacture/solver.rs:62-164`），无该效果时忽略 `shift_hours`（`:67-68`），另有 `manufacture_combo_allowed_for_shift` 的班时硬拒绝（`manufacture_fill.rs:460`）；发电充能斜坡用 `eff_ramp_at_shift_hours`，空构上限于 20 小时（`power/solver.rs:110-125`）。`ShiftEta` 对整份 assignment 一次计算（`mood/eta.rs:32-44、54-70`），红脸时间读中枢满员与中枢干员（`eta.rs:119-136`）。中枢选人先于贸易/制造填充（`pipeline.rs:188-260、1202-1278`），只读布局、中枢池与 plan 要求，不读房间候选人，因此当前流程不构成环；现有顺序是"中枢先于房间 + 终态复核"（`assign.rs:342/371/403`）。写入前仍要核实：若把整份 assignment 的每房 ETA 放进候选比较，必须先固定中枢，且不能反过来用房间候选重选中枢。
4. 门禁是整块的。`assert_final_rotation_invariants_with_mood`（`team_rotation.rs:4863`）以 `TeamRotationReport` 为输入；现有主备互斥检查只有 `assert_main_backup_24_24_final_state`（`:6274`，其中 `main_backup_24_24_work_disjoint` 在 `:6434`），只覆盖 24/24 一个 profile，没有对任意主备两态的通用检查。需要新增针对自动轮换的最终态检查；批量脚本已通过 `--rotation` 透传（`scripts/assert_rotation_invariants.py:86、127`），新入口只需透传标记，不重写机制。
5. 周期心情硬门禁：已由 2026-10-28 结构化问答关闭——不做硬门禁、不模拟恢复，只要求满心情能上满评估时长（见第三节第 7 项）。

### 2.4 设计（拟）

- 独立类型：新增 `AutoRotationReport`（每房主力、替补、效率、评估时长、自动化关联、宿舍挂件常驻说明、缺员位），不进入 `export/maa.rs`，不进入 `BoxProfile`。
- 入口：先挂 `layout auto-rotation` 子命令（只排班，不出 profile 与 MAA，与 `layout team-rotation` 同级）。Worker 方法与 schema 版本待用户需要网站时再定。
- 禁令实现为"准入层 + 最终态断言"两层：准入层在候选进入处过滤；断言层在最终 assignment 上检查，作为 hard constraint。分类表由 `data/` 中显式条目承载，每个体系、规则、alternative 都要标注，缺项即失败。
- 时长：按已裁决原话，时长进入候选比较；写入前先核实 `shift_hours` 的实际影响面（见裁决 8）。
- 保证等级（agent 建议默认，用户未逐字裁决）：禁令为 hard constraint；选人为 heuristic；时长评估为 policy。不声称全局最优；用户已表示“我们也从来没有保证全局最优”（2026-10-27，`e235e78e` L3369）。

## 三、需要用户裁决的事项

每项给出建议默认值；标为“已裁决”的按用户原话执行，其余默认值只是实现者建议，用户未确认前不得写成已裁决规则。

| # | 事项 | 影响 | 建议默认 |
|---|---|---|---|
| 1 | 四赤金比较窗口 | 决定读取点与聚合方式 | **已裁决（2026-10-26，原话“比较窗口这里，直接用单站比较就够了”，`838409c4` L1698）：单站 `final_efficiency`，不折整轮周期** |
| 2 | 多贸易站聚合 | 每个候选按其落位规则填站；四赤金按 1/2/3 站形态填 | **agent 默认，用户未逐字裁决**：逐站取 `final_efficiency`，按候选占用的赤金贸易站求和后比较；同一组金单站上求和与平均排序一致。用户只说过“单站比较”与“平手输出n”，没有逐字裁决聚合方式 |
| 3 | 是否与菲亚 N/A/B 叠加比较 | 单站比较发生在 Plan 编译期，与菲亚候选层无关 | **建议默认（未裁决）**：不再适用（仅备选重方案需要） |
| 4 | 平手 | `Efficiency` 是整数，平手是精确相等 | **已裁决（2026-10-28，原话“按固定候选顺序取第一个”，`0b5face2`，2026-10-28 结构化问答）：所有候选排固定、显式、不依赖哈希或运行顺序的顺序表，平手取排在前面的**；取代 2026-10-26 的“平手输出 N（普通方案）”读法，具体顺序表见四赤金设计文档 §4.5（agent 建议默认、实现时固定） |
| 5 | 是否同时要求制造和发电不变差 | 杜林占位可能影响制造 | **已裁决（2026-10-28）：总分只看贸易**（原话“总分只看贸易就可以”，`e235e78e` L5434）；制造与发电差值是否作为诊断输出仍是建议默认 |
| 6 | 2026-09-28 "只要使用，必定优先分房使用" 与 ADR 0013 "与普通完整候选竞争" | 前者是落位约束，后者是候选竞争 | **已澄清（2026-10-26，原话“不同的trade_core要竞争效率的”“没有优先级区别”，`838409c4` L2252/L2272）：所有 trade_core 正常选人竞争，无优先级差别；"优先分房"只约束四赤金候选自己的三人落位，不压过其他核心** |
| 7 | 自动轮换是否适用周期心情硬门禁 | 主备各一班时该检查近似恒真 | **已裁决（2026-10-28，原话“按照我们的思路试算就可以，不需要考虑恢复，只要满心情能上满时间就可以”，`0b5face2`，2026-10-28 结构化问答）：不做周期心情硬门禁、不模拟恢复；只要求每组从满心情出发能上满评估时长（首人红脸或 36 小时中较早者）** |
| 8 | 自动轮换的红脸时长：进入候选比较，还是先保守选人、终态按每房时长评估 | **已裁决（2026-10-11 第 1 题答 1A）：按组内首人红脸与 36 小时中较早者计算**；“必须进入候选比较”是 agent 对该口径的实现解读。已核对（`db0755b`）：贸易效率不随班时变化，`shift_hours` 只影响产量/日换算（`trade/solver.rs:53-56、231、246`）；制造按实际班时分段平均（`manufacture/solver.rs:62-164`）并有班时硬拒绝（`manufacture_fill.rs:460`）；发电斜坡按 `eff_ramp_at_shift_hours`、20 小时上限（`power/solver.rs:110-125`）；中枢选人先于房间填充（`pipeline.rs:188-260、1202-1278`），当前流程不构成环 | 按已裁决原话执行（1A）；仅当写入前核实改动面不小时，再请用户确认是否降级为终态评估 |
| 9 | 自动化中需要中枢森蚺的分支，以及莱茵宿舍 1+1 | 放行范围 | **已裁决（2026-10-28）：按 ADR 建议默认执行——只放行发电＋制造分支；莱茵 1+1 按"一挂件一受益者"放行，其余拒绝**（原话“按 ADR 建议默认 (Recommended)”，`0b5face2`，2026-10-28 结构化问答；放行清单是本项原建议默认内容，经用户答句采纳） |
| 10 | 是否提交 4 金线 layout fixture | 无 fixture 就没有四赤金回归证据 | **建议默认（未裁决）**：提交 243 与 342 两份 layout（仅数据） |
| 11 | 自动轮换中是否可用四赤金 | 四赤金依赖全基建杜林人数和跨贸易房的虚拟赤金线 | **已裁决（2026-10-28）：禁用** |

第 1 项已由用户 2026-10-26 裁决；第 4 项的平手键由 2026-10-28 裁决更新为“按固定候选顺序取第一个”；第 6 项为 2026-10-26 澄清；第 2 项是 agent 默认，用户只说过“单站比较”与“平手输出n”，没有逐字裁决聚合方式；第 5 项由 2026-10-28 原话“总分只看贸易就可以”落实；第 7、9 项由 2026-10-28 结构化问答裁决；第 8 项的计算口径由 2026-10-11 第 1 题答 1A 裁决；第 11 项为 2026-10-28 裁决。ADR 0013 D4 的比较窗口与平手两项据此落定；聚合项按 agent 默认执行，转 accepted 前需用户确认。D3 的“必须比较完整 Plan”按单站比较收窄。

行号更正（2026-10-28）：外部设计文档曾以 `0014:185` 引用“收窄为单站比较”，该行号有误——本 ADR 记录该收窄的是 §1.4 末条与本节（原文第 86、158 行；第 185 行是代码块围栏）。本文改动后行号会移动，后续引用请用节号，不用行号。

## 四、实现路线

每个写入单元只有一个主要意图。每个源码单元开始前，先按 AGENTS.md 向用户说明输入、状态改变顺序、输出、保持不变的行为、允许变化和验证场景。与 r24 的 ABC `daily/warnings` 汇总清理共用 `team_rotation.rs`，所以排在其后，并保持单写者。

### 四赤金

1. **U1 fixture（纯数据）**：提交 243 与 342 的 4 金线 layout，并记录所用 Box。不改行为。
2. **U2 跳过钩子**：`AssignBaseOptions` 增加体系级跳过集合并并入 `assign_shift_with_preclaimed_plan`。默认空，对现有输出无影响。
3. **U3 trade_core 候选竞争**：把四赤金（含恢复的 1 站同房形态）、可露希尔核心、巫恋组、但书核心放进同一个 trade_core 候选比较层；各自按固定落位规则填站，逐站取 `final_efficiency` 求和选最高，平手按固定候选顺序取第一个（2026-10-28 裁决）；撤销四赤金 priority 1001 的压过行为。四赤金候选按假定 4 名杜林评估，胜出后才由挂件模块放置宿舍挂件（2026-10-28 裁决，实现方式待核实）。依赖裁决 1、2、4、5、6。
4. **U4 证据**：同 Box、同 layout 的开 / 关 A/B；在 `high-owned/b-101-424.json` 与 `243/operbox_full_e2.json` 上留存双方完整最终 assignment、度量和选择理由。
5. **U5 门禁**：在 Rust 最终 assignment 上加自洽检查（报告所记选择与最终 Plan 一致，且符合所裁决的比较规则），Python 批量对代表与 high-owned 集合只调用该 gate。

### 自动轮换

本轮与四赤金并行开发（2026-10-28 结构化问答答“并行 (Recommended)”），分两个 worktree。阶段 1（四赤金）合入后再做 U8 接线。

1. **U6 文档**：更新 `docs/排班模式.md` 第 17、30 行与 `docs/GLOSSARY.md` 第 110 行（"按组合耗尽触发" 与已裁决的 "首人红脸或 36 小时" 不一致），新建自动轮换 canonical 语义页 `docs/自动轮换.md`（领域键 `schedule.auto_rotation`）。
2. **U7 分类表**：显式列出每个体系、规则、alternative、角色与 producer 的"允许 / 禁止 / 委托"，缺项 fail-closed。
3. **U8 准入接线**：把 U7 接到所有候选入口，并加最终态断言。**前置条件（2026-10-28 计划）**：阶段 1（四赤金 trade_core 枚举重构）会删除补位梯子中的但书/可露希尔/巫恋步骤与鸿雪+图耶同房特判；U8 在阶段 1 合入后再接线，不给这些路径接线。分类表仍须覆盖其余入口。
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
- 已有文档矛盾的统一整理（`docs/TODO/排班统一内核重构方案.md` 第 714 行与 781、869 行；`docs/调用链/plan请求.md` 第 60 行与 62 行）。它们只在此处登记，不在本 ADR 中修改。

## 后果

- 本 ADR 为 `proposed`。用户确认第三节后，才能转为 `accepted` 并启动 U3；U1、U2、U6、U7 不依赖第 1–5 项裁决。
- 四赤金的完成标准以 ADR 0013"后果与验收边界"为准，本 ADR 不放宽。
- 本 ADR 不改变 ADR 0012 的状态判定。

## 依据

- [ADR 0012](0012-october-rotation-delivery-boundaries.md) 的第 45、47 行与关闭边界 2、4。
- [ADR 0013](0013-four-gold-trade-core-candidate.md) 的 D3、D4。
- [贸易求解](../模块/贸易求解.md) 与 [plan 请求调用链](../调用链/plan请求.md) 中关于首个可行候选与缺失比较器的记录。
- [排班模式](../排班模式.md)、[排班统一内核重构方案](../TODO/排班统一内核重构方案.md) 的 r24 检查点。
- 自动轮换用户原话与裁决：`target/codex-runs/opencode-six-topics-20261019/readable/auto-rotation.md` 与 `auto-rotation.users.md`（本地证据，不入库）；2026-10-24 补充见会话 `1d9851f1` L3；2026-10-28 结构化问答见会话 `0b5face2` L963。
- 四赤金新设计：`docs/TODO/四赤金与贸易站核心枚举设计.md`（另一工作树同期撰写，状态 `proposal`；本 ADR 第一节仅保留历史）。
