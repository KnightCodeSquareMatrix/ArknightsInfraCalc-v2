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

中枢容量由 blueprint 决定，当前标准布局为 5 人。可用角色足够时先满足 Plan 的 required anchor，再补齐合法席位；无中枢技能的 filler 仍可能提供 occupancy/mood 或满足跨设施条件。后续 filler 不能替换已认领的 required anchor。定时轮换内部班次可在候选构造阶段暂时少于容量，required anchor、`min_count`、身份唯一性、活跃队伍准入与练度仍需满足；完整周期后的普通 fallback 再从当前合法剩余候选补位。候选耗尽时最终 Assignment 保留缺额并由缺员 warning 报告，不把单纯容量不足升级为产品拒绝；超容量、重复 operator、同房禁配和无法满足仍不可降级的 required group 继续错误/无解。最终显式 invariant 核验中枢不超容量，并重放同一普通补位策略：仍有合法候选可加入才失败，候选耗尽后的短员保留 warning，不以满员作为通过前提。其余门禁按各自已实现范围独立执行，不据此声称所有 profile 的 required 或通用绑定均已独立重放。

`search/control.rs` 的 `ControlFillPolicy` 是顺序 policy，不是全基建匿名总分。排序依次消费 Plan anchor、贸易/制造全局区域、训练/办公室/会客室、心情去重和普通补位；不回溯替换前序结果。

定时轮换在进入班次循环前冻结 `ControlRotationClosure`：候选身份、所属 cohort、shared/required、候选组来源和适用班次由 Plan、实际 peak assignment、bind 和真实占用共同确定。每班的 `ControlShiftAdmission` 只把当前活跃队伍和当班特殊分支交给 fill；普通候选不会从全账号池重新归队。`ControlCandidateRequirement` 通过有序 registry claim/slot 保留来源，`min_count` 在满员快捷路径、替换和最终 assignment 上统一复核。

心情补位消费 `solve_control` 的当前前缀结算和既有 MoodModel 作用范围；不同设施的统一减免不转换成个人耗损分数。最终心情/办公室条件使用中枢结算前快照，办公室后置求值不能覆盖中枢条件输入。未建模技能保留为 pool skip，不擦除 buff 后作为 filler。

魔王两档同房配对恢复明确委托给 `MoodModel` 的 `control_pair_recovery` 数据与净消耗 owner，不由 control atom 再结算一次。显式承认配对技能来源后，E0 魔王可进入正常中枢候选池；精二档还包含既有 `期冀之汇` 来源，因此不再因配对 buff 缺映射而整人 skip，该 +15% 会客室线索效果仍沿既有中枢账本结算，不是配对恢复的第二份结算。未知技能的 pool skip 规则不变。配对读取当前实际中枢房间成员和来源练度，只影响指定两人，不属于统一全中枢恢复或可扩散的笑脸。条件、档位和数值引用固定知识库[控制中枢技能](知识库引用.md)中的魔王章节；候选预算、周期检查与最终 gate 共用实际心情 owner。接入不新增强制配队、跨班 bind 或选位政策，合法入池不等于必然入选。

## 体系准入档位

体系准入按技能实际解锁档位执行，不以完整形态档代替。怪猎组合（火龙S黑角＋麒麟R夜刀进驻控制中枢、泰拉大陆调查团消费木天蓼；登记项 `snhunt_monhun_control` / `snhunt_monhun_control_factory`）的「团队合作」「耐力回复」与「可爱的艾露猫」为精0/初始解锁，贸易变体按精0准入；「可靠的随从们」为 30 级解锁，制造变体按 `min_level: 30` 准入，运行档位同口径（`tier_0` 不含制造 buff）。两条变体均由 `bind_all` 保持三人同班。精2「秘传交涉术」（全贸易+7%）与「以身作则」（全制造+2%）随档位自然结算，不构成准入条件。知识库《怪猎中枢》的"双精2"按含光环的完整形态理解，产品准入与光环结算在该组合上分离。

## Ledger 事实

`ControlEffectKey {domain, unit}` 是结构化 key。`record_max` 记录候选来源，`freeze` 对同 key 取最大值，并标记 `Winner`、`TiedWinner` 或 `Suppressed`。注入、trace、candidate dependency 和投影视图都应读取冻结 ledger；不得按 operator 名、buff 前缀或数值单位猜同类效果。冲突来源仍是物理占位冲突，即使低值 atom 已 suppressed。

`solve_control_with_trace` 返回 `ControlCenterResult` 和 `ControlSettlementTrace`；`apply_control_to_layout_with_trace` 把 global pool、`GlobalInjectManifest`、办公室和会客室速度写回 `LayoutContext`。下游 trade/manufacture 只消费 resolved facts。

## 动态 producer

中枢 producer 的 admission、target、relation 由 `data/producer_rules.json`/Plan 解析；source 只有实际 assignment 在场时才进入 manifest。producer 变化应触发 resolve 和下一阶段搜索，不能靠标签存在或 export 补偿。

## 灵知可选协同（2026-10-30）

用户裁决采用部分搜索：“它的队友落选就丢弃掉”。灵知不再由 `ling_jie_karlan` registry 提前认领中枢、孑和喀兰成员，旧 registry 及其生成脚本模板已删除。`trade_karlan_precision` 在 producer 目录声明为 deferred optional；普通中枢补位仍排除该技能，仅显式试算分支或既有硬 pin 可以进入。

Peak 复用动态中枢 presence 枚举，每个分支按当前贸易 role/房间顺序选人，不固定孑、银灰或第三人。移除喀兰 role 可用就优先返回的补位路径；贸易填完后，实际同房没有市井孑与喀兰成员的可选灵知分支直接淘汰，不重排其他房间、不回头抢队友。实际 Box 练度固定。第三人不要求喀兰；缺银灰的低档合法协同也不按技能非法排除。

`ControlInjectRawSumV0` 保留原注入比较项，并加入灵知在完整实际贸易名单上的边际 final 效率百分点：同一名单分别由贸易 solver 按有/无精密计算结算，差值求和。该差值不是队友重新优化收益，也不代表全基建机会成本；`TradeManufactureWeightedEfficiencyV1` 保留原比较口径。旧正注入上界不覆盖此交互，有精密计算的分支不走该上界剪枝。整体仍是 **heuristic / 部分搜索**，不声称全局最优。

胜出名单由 `resolve_actual_producer_dependencies` 派生 `trade_karlan_precision` 实际依赖，精密计算来源与实际市井孑、喀兰受益成员沿既有 exact-presence 同班；同站关系随既有生产组件轮换。独立第三人（如但书、可露希尔）不额外绑同休，可按其原有连续工作或菲亚规则继续在岗。夜烟归属重算若破坏该已选协同，其替代结果不提交。终态 gate 的 `karlan_precision_selected_team` 在最终 assignment 检查在岗来源对应的实际贸易成员同房，既有 `karlan_trade_requires_control_source` 和 bind cycle 检查反向来源与同班。旧 `scripts/check_karlan_rotation.py` 入口转调统一批量 runner，始终运行 Rust 终态 gate，不再用 Python 重写 registry 认领或上下班判断。

## 核验

检查 control trace、ledger records、global inject explanation、最终 assignment invariant。固定候选用 `layout eval --explain-json`；整班用 `plan --assert-invariants` 和 243/high-owned fixture。中枢技能原文与同种效果规则回知识库引用，本文只记录实现责任和状态流。
