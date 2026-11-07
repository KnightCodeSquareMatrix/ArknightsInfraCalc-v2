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

中枢容量由 blueprint 决定，标准布局为5人。required anchor先落位，后续filler不替换。普通候选与初始Plan Rule/registry认领在最初中枢建池/承诺入口使用既有单人保守ETA；ABC按队伍归属窗口首次冻结。动态producer、满pin、普通补位等后续阶段不再复筛工作预算；技能、占用、Team/Shift、容量与Plan来源仍独立生效。深海宿管按真实恢复量排序，不以恢复量能否覆盖工作量淘汰。构造候选不足或承诺矛盾保留诊断和此前岗位，继续普通完成；终检不修admission。显式gate检查容量、唯一性、required/min_count和绑定，并核对真实周期心情提示准确性；心情不足本身及候选耗尽后的短员不升级为硬失败。当前裁决见[定时换班](../排班/定时换班.md)及[删除交付](../ARCHIVE/done/后续心情筛选删除与排班耗时-20261031.md)。

`search/control.rs` 的 `ControlFillPolicy` 是顺序 policy，不是全基建匿名总分。排序依次消费 Plan anchor、贸易/制造全局区域、训练/办公室/会客室、心情去重和普通补位；不回溯替换前序结果。

定时轮换在进入班次循环前冻结 `ControlRotationClosure`：候选身份、所属 cohort、shared/required、候选组来源和适用班次由 Plan、实际 peak assignment、bind 和真实占用共同确定。每班的 `ControlShiftAdmission` 只把当前活跃队伍和当班特殊分支交给 fill；普通候选不会从全账号池重新归队。`ControlCandidateRequirement` 通过有序 registry claim/slot 保留来源，`min_count` 在满员快捷路径、替换和最终 assignment 上统一复核。

心情补位消费 `solve_control` 的当前前缀结算和既有 MoodModel 作用范围；不同设施的统一减免不转换成个人耗损分数。最终心情/办公室条件使用中枢结算前快照，办公室后置求值不能覆盖中枢条件输入。未建模技能保留为 pool skip，不擦除 buff 后作为 filler。

魔王两档同房配对恢复明确委托给 `MoodModel` 的 `control_pair_recovery` 数据与净消耗 owner，不由 control atom 再结算一次。显式承认配对技能来源后，E0 魔王可进入正常中枢候选池；精二档还包含既有 `期冀之汇` 来源，因此不再因配对 buff 缺映射而整人 skip，该 +15% 会客室线索效果仍沿既有中枢账本结算，不是配对恢复的第二份结算。未知技能的 pool skip 规则不变。配对读取当前实际中枢房间成员和来源练度，只影响指定两人，不属于统一全中枢恢复或可扩散的笑脸。条件、档位和数值引用固定知识库[控制中枢技能](知识库引用.md)中的魔王章节；初次中枢ETA、最终周期诊断和gate提示核对共用实际心情owner；后续候选不按工作预算复筛。接入不新增强制配队、跨班 bind 或选位政策，合法入池不等于必然入选。

## 体系准入档位

体系准入按技能实际解锁档位执行，不以完整形态档代替。怪猎组合（火龙S黑角＋麒麟R夜刀进驻控制中枢、泰拉大陆调查团消费木天蓼；登记项 `snhunt_monhun_control` / `snhunt_monhun_control_factory`）的「团队合作」「耐力回复」与「可爱的艾露猫」为精0/初始解锁，贸易变体按精0准入；「可靠的随从们」为 30 级解锁，制造变体按 `min_level: 30` 准入，运行档位同口径（`tier_0` 不含制造 buff）。两条变体均由 `bind_all` 保持三人同班。精2「秘传交涉术」（全贸易+7%）与「以身作则」（全制造+2%）随档位自然结算，不构成准入条件。知识库《怪猎中枢》的"双精2"按含光环的完整形态理解，产品准入与光环结算在该组合上分离。

## 怪猎两席延迟选入（2026-11-05）

用户裁决沿优先级串行路径处理，并在贸易搜索结束后直接读取实际二人／三人；前序选位后中枢有两席余位时可以延迟绑定。怪猎贸易候选所需的真实木天蓼来源可在该两席暂时在岗，但暂时来源不是最终registry认领或三人绑定。实际调查团在贸易消费岗位、或已在固定制造槽且达到30级时，保留三人形态；调查团贸易落选且无制造消费时，有效龙门先于怪猎二人纯光环。

延迟路径不替换前序感知／显式取向成员，不增加贸易重搜；最终Plan认领和activated来源必须同步为实际选择，再派生同班绑定。两席不足则不进入双人延迟路径。此改动为policy顺序调整，不构成全局最优保证。决策与实施验收状态见[ADR 0016](../ADR/0016-怪猎低档协同与中枢光环选入.md)。

后续夜烟归属处理不得破坏已确定三人形态的实际调查团消费岗位；替代结果不满足该事实时不提交，保留已完成的生产名单，不重新搜索或降级已提交中枢。

## Ledger 事实

`ControlEffectKey {domain, unit}` 是结构化 key。`record_max` 记录候选来源，`freeze` 对同 key 取最大值，并标记 `Winner`、`TiedWinner` 或 `Suppressed`。注入、trace、candidate dependency 和投影视图都应读取冻结 ledger；不得按 operator 名、buff 前缀或数值单位猜同类效果。冲突来源仍是物理占位冲突，即使低值 atom 已 suppressed。

`solve_control_with_trace` 返回 `ControlCenterResult` 和 `ControlSettlementTrace`；`apply_control_to_layout_with_trace` 把 global pool、`GlobalInjectManifest`、办公室和会客室速度写回 `LayoutContext`。下游 trade/manufacture 只消费 resolved facts。

## 动态 producer

中枢 producer 的 admission、target、relation 由 `data/producer_rules.json`/Plan 解析；动态mask从已完成初次ETA筛选的control pool枚举required，不在mask或落位阶段重算工作预算。source 只有实际 assignment 在场时才进入 manifest，producer 变化触发 resolve 和下阶段搜索。当前 selector 不生成旧 ProducerSlot；该接口若被使用仍检查恢复owner冲突与占用，不复筛工作预算，不构成免准入入口。

## 灵知可选协同（2026-10-30）

用户裁决采用部分搜索：“它的队友落选就丢弃掉”。灵知不再由 `ling_jie_karlan` registry 提前认领中枢、孑和喀兰成员，旧 registry 及其生成脚本模板已删除。`trade_karlan_precision` 在 producer 目录声明为 deferred optional；普通中枢补位仍排除该技能，仅显式试算分支或既有硬 pin 可以进入。

Peak 复用动态中枢 presence 枚举，每个分支按当前贸易 role/房间顺序选人，不固定孑、银灰或第三人。移除喀兰 role 可用就优先返回的补位路径；贸易填完后，实际同房没有市井孑与喀兰成员的可选灵知分支直接淘汰，不重排其他房间、不回头抢队友。实际 Box 练度固定。第三人不要求喀兰；缺银灰的低档合法协同也不按技能非法排除。

`ControlInjectRawSumV0` 保留原注入比较项，并加入灵知在完整实际贸易名单上的边际 final 效率百分点：同一名单分别由贸易 solver 按有/无精密计算结算，差值求和。该差值不是队友重新优化收益，也不代表全基建机会成本；`TradeManufactureWeightedEfficiencyV1` 保留原比较口径。旧正注入上界不覆盖此交互，有精密计算的分支不走该上界剪枝。整体仍是 **heuristic / 部分搜索**，不声称全局最优。

胜出名单由 `resolve_actual_producer_dependencies` 派生 `trade_karlan_precision` 实际依赖，精密计算来源与实际市井孑、喀兰受益成员沿既有 exact-presence 同班；同站关系随既有生产组件轮换。独立第三人（如但书、可露希尔）不额外绑同休，可按其原有连续工作或菲亚规则继续在岗。夜烟归属重算若破坏该已选协同，其替代结果不提交。终态 gate 的 `karlan_precision_selected_team` 在最终 assignment 检查在岗来源对应的实际贸易成员同房，既有 `karlan_trade_requires_control_source` 和 bind cycle 检查反向来源与同班。旧 `scripts/check_karlan_rotation.py` 入口转调统一批量 runner，始终运行 Rust 终态 gate，不再用 Python 重写 registry 认领或上下班判断。

## 核验

普通 ABC 中枢选位前，办公室 owner 按本班合法候选和逐班预算实际落位；`resolve_control_input_layout` 从这些已提交人员取得联络速度，交给既有琴柳选位条件。中枢选人消费办公室占用，不能再抢走该成员；不保留另一套办公室预测名单。技能阈值仍只由原中枢解释器结算。

办公室先落位属于顺序 policy，不能据此声称办公室与中枢联合选人的全局最优。辅助岗位的终态心情预算是已确认的硬约束；构造时的排序、试放顺序和普通补位仍是各自具名 policy。

检查 control trace、ledger records、global inject explanation、最终 assignment invariant。固定候选用 `layout eval --explain-json`；整班用 `plan --assert-invariants` 和 243/high-owned fixture。中枢技能原文与同种效果规则回知识库引用，本文只记录实现责任和状态流。
