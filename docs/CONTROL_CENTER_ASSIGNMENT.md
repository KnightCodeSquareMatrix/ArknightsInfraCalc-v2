# 控制中枢编制与效果账本

> 文档角色：canonical
> 生命周期状态：current
> 摘要：说明控制中枢的 required anchor、顺序选位、EffectLedger 和全局注入责任边界。
> 领域键：facility.control-assignment
> 当前真源：self
> 代码范围：`crates/infra-core/src/control/{input,interpreter,ledger,mod}.rs`; `crates/infra-core/src/search/control.rs`; `crates/infra-core/src/pool/control.rs`; `crates/infra-core/src/layout/assign/{control_fill,pipeline}.rs`; `crates/infra-core/src/global_resource/inject.rs`; `crates/infra-core/src/layout/system.rs`; `data/base_systems.json`
> 代码锚点：`crates/infra-core/src/control/interpreter.rs::{solve_control,solve_control_with_trace}`; `crates/infra-core/src/control/ledger.rs::ControlEffectLedger`; `crates/infra-core/src/control/mod.rs::apply_control_to_layout_with_trace`; `crates/infra-core/src/search/control.rs::search_control_combos`; `crates/infra-core/src/layout/assign/control_fill.rs::assign_control`; `crates/infra-core/src/layout/assign/pipeline.rs::run_shift_pipeline`
> 关联链路：`docs/模块/中枢与跨设施状态.md`; `docs/调用链/跨设施状态刷新.md`; `docs/模块/候选搜索与Bake.md`
> 核验基线：`947f1a2`

## 编制硬约束

中枢容量由 blueprint 决定，当前标准布局为 5 人。可用角色足够时先满足 Plan 的 required anchor，再补齐合法席位；无中枢技能的 filler 仍可能提供 occupancy/mood 或满足跨设施条件。后续 filler 不能替换已认领的 required anchor。缺席、重复 operator、同房禁配和无法满足 required group 返回错误/无解。

`search/control.rs` 的 `ControlFillPolicy` 是顺序 policy，不是全基建匿名总分。排序依次消费 Plan anchor、贸易/制造全局区域、训练/办公室/会客室、心情去重和普通补位；不回溯替换前序结果。

定时轮换在进入班次循环前冻结 `ControlRotationClosure`：候选身份、所属 cohort、shared/required、候选组来源和适用班次由 Plan、实际 peak assignment、bind 和真实占用共同确定。每班的 `ControlShiftAdmission` 只把当前活跃队伍和当班特殊分支交给 fill；普通候选不会从全账号池重新归队。`ControlCandidateRequirement` 通过有序 registry claim/slot 保留来源，`min_count` 在满员快捷路径、替换和最终 assignment 上统一复核。

心情补位消费 `solve_control` 的当前前缀结算和既有 MoodModel 作用范围；不同设施的统一减免不转换成个人耗损分数。最终心情/办公室条件使用中枢结算前快照，办公室后置求值不能覆盖中枢条件输入。未建模技能保留为 pool skip，不擦除 buff 后作为 filler。

## 体系准入档位

体系准入按技能实际解锁档位执行，不以完整形态档代替。怪猎组合（火龙S黑角＋麒麟R夜刀进驻控制中枢、泰拉大陆调查团消费木天蓼；登记项 `snhunt_monhun_control` / `snhunt_monhun_control_factory`）的「团队合作」「耐力回复」与「可爱的艾露猫」为精0/初始解锁，贸易变体按精0准入；「可靠的随从们」为 30 级解锁，制造变体按 `min_level: 30` 准入，运行档位同口径（`tier_0` 不含制造 buff）。两条变体均由 `bind_all` 保持三人同班。精2「秘传交涉术」（全贸易+7%）与「以身作则」（全制造+2%）随档位自然结算，不构成准入条件。知识库《怪猎中枢》的"双精2"按含光环的完整形态理解，产品准入与光环结算在该组合上分离。

## Ledger 事实

`ControlEffectKey {domain, unit}` 是结构化 key。`record_max` 记录候选来源，`freeze` 对同 key 取最大值，并标记 `Winner`、`TiedWinner` 或 `Suppressed`。注入、trace、candidate dependency 和投影视图都应读取冻结 ledger；不得按 operator 名、buff 前缀或数值单位猜同类效果。冲突来源仍是物理占位冲突，即使低值 atom 已 suppressed。

`solve_control_with_trace` 返回 `ControlCenterResult` 和 `ControlSettlementTrace`；`apply_control_to_layout_with_trace` 把 global pool、`GlobalInjectManifest`、办公室和会客室速度写回 `LayoutContext`。下游 trade/manufacture 只消费 resolved facts。

## 动态 producer

中枢 producer 的 admission、target、relation 由 `data/producer_rules.json`/Plan 解析；source 只有实际 assignment 在场时才进入 manifest。producer 变化应触发 resolve 和下一阶段搜索，不能靠标签存在或 export 补偿。

## 核验

检查 control trace、ledger records、global inject explanation、最终 assignment invariant。固定候选用 `layout eval --explain-json`；整班用 `plan --assert-invariants` 和 243/high-owned fixture。中枢技能原文与同种效果规则回知识库引用，本文只记录实现责任和状态流。
