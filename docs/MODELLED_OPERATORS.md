# 已建模干员

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/EFFECT_ATOM_DESIGN.md；docs/MANUFACTURE_STATUS.md
> 摘要：列出已建模干员和机制摘要

> 从 [`EFFECT_ATOM_DESIGN.md`](EFFECT_ATOM_DESIGN.md) §4 抽出。每新增干员时追加到本文末尾。

---

## 4.1 但书

| Tier | 技能 | EffectAtom |
|------|------|------------|
| 通用 | 合同法 | Condition: `GoldDeliveryBelow(4)` → Action: `TagOrder("breach")` |
| Tier0 | 违约索赔·α | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(1)` |
| TierUp | 违约索赔·β | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(2)` |

L2 产量：`for_trade_level` 裁剪订单档位；违约链在 2/3 金订单上叠加 `DOCUS_SUB4_LMD_BONUS`（与工具人表 2/3 金 +1000 对齐，校准见 `UNIT_OUTPUT_ANCHORS.csv`）。

**排班约束**：但书**单走一站**（但书 + 订单效率工具人），精0和精二都可作为主力 core；精0仅使用 α、精二使用 β。除 153 外，但书 core 优先于可露希尔并选择正式搜索的高效率队友；完整龙巫仍要求巫恋、龙舌兰和对应裁缝组合，不与但书同房。

**L3 但书单走**（`gsl_docus_solo`）：社区单位日产出按等级写入 `unit_output`；实际产出使用 `total_efficiency × unit_output_per_day × 工作时间占比`。`mechanic_equivalent_efficiency=0.550` 只作机制解释，不二次参与计算。

## 4.2 可露希尔

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 总工程师 | 心情恢复（中枢，不在模拟范围） |
| TierUp | 特别订单 | Action: `AddFlatEff(10.0)` + Action: `TagOrder("closure_special")` |

特别订单：2:24:00，交付 2 赤金，1200 龙门币。不视作违约订单。

## 4.3 孑

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 摊贩经济 | Action: `AddPerGapEff(4.0)` |
| TierUp | 摊贩经济 + 市井之道 | 保留 Tier0 `AddPerGapEff(4.0)`；Step1: `OtherOpsSettledEff` → `ReduceLimit(floor(eff/10))` |
| | | Step2: `OrderCount` → `AddFlatEffFromSelector(×4.0)`，`phase=order_var` |

**精0 摊贩**为无灵知时的贸易常用态（`AddPerGapEff`，依赖 order_gap）。用户裁决孑精 1+ 的两个技能同时生效，因此 **精1+ 市井同时保留摊贩经济**；该形态默认不进通用贸易池，中枢**灵知 E2·精密计算**激活时注入双技能孑，由 L1 搜索与喀兰队友自然上浮。

`OrderCount` 读取实际当前订单数并 clamp 至最终上限。孑精1+双技能共存时，摊贩经济读取 `final_order_limit - order_count`，市井之道读取 `order_count`，两项合计恒为 `final_order_limit × 4%`，不得把市井再次按最终上限计算而重复叠加差额。

与雪雉·天道酬勤：`Condition::TiandaoEffVarAllowed` — 同房仅有孑+雪雉且无第三方 settled 时，天道酬勤不生效（市井之道优先）；有第三方贡献时两者均生效。

L1 自然 winner 为中枢灵知 E2 + 贸易站双技能孑 / 银灰 / 琳琅诗怀雅：技能效率 `129%`，三人基础效率 `103%`，总效率 `2.320`，且 `trade_shortcut=None`。拆法：银灰受精密计算后 `5%`，琳琅固定 `20%`，孑双技能按最终 18 单合计 `72%`，琳琅按超出 10 单的 8 单获得 `32%`。`gsl_ling_jie_yaxin` 只保留为参考锚点，不参与 active L3 匹配。

### 4.3.1 灵知·精密计算（跨设施 → 贸易房）

| Tier | 技能 | EffectAtom |
|------|------|------------|
| TierUp | 精密计算 | `Action::GlobalInjectKarlanPrecision { eff_per_karlan: -15, limit_per_karlan: 6 }`，`phase=global_inject` |

控制域写入 `GlobalInjectManifest::karlan_precision`；贸易域 `TradeContext::seed_karlan_precision()` 在相位前对同房 **`cc.g.karlan` 干员**写入 settled_eff / limit_contrib，使市井 `ReduceLimit` 读到被 debuff 后的 `other_ops_settled_eff`。孑与琳琅诗怀雅不带该 tag，不吃精密计算。

**非目标**：灵知 E0「幕后指挥」心情恢复（`control_mp_cost&faction[030]`）。

## 4.5 雪雉

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 天道酬勤·α | Condition: `TiandaoEffVarAllowed` → `PeerSettledEffSum` → `AddBucketEffFromSelector(5/5, cap 25)` |
| TierUp | 天道酬勤·β | 同上，cap 35 |

## 4.7 巫恋

| Tier | 技能 | EffectAtom |
|------|------|------------|
| TierUp | 低语 | `PeerEffAbsorb(45)` + 全体 `MoodDrainDelta(+0.25)` |

与佩佩共用 `PeerEffAbsorb` 原语；巫恋 `rate_per_peer=45`，佩佩 `rate=0`（只清零、不吸收）。

**编排**：`trade_segments.roles.witch` 强制包含精二巫恋、持有即可的龙舌兰（精0/精1/精2）和合法龙巫候选；精二龙舌兰优先使用裁缝 β/α 路径，低练度龙舌兰允许 `gsl_witch_long0_alpha` 或 `gsl_witch_long0_blank` 排班路径。普通白板第三人不得伪造成完整高练度龙巫候选；`witch_long_beta` 不作为主路径 fixed registry 早占站。此处是排班 admission 规则，不改变练卡推荐口径。

## 4.6 佩佩

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 多面逢源 | `FacilityLevel` → `AddLimitFromSelector(×1)` |
| TierUp | 慧眼独到 | `PeerEffAbsorb(0)` + `TagOrder("pepe_exclusive")` |

**L2 特别独占订单**：4:30:00，0 赤金，1000 龙门币；不视作违约；**不受任何订单获取效率影响**。

## 4.4 凯尔希 / 思衡托（跨房间）

| 干员 | EffectAtom |
|------|------------|
| 凯尔希 | Condition: `MoodAbove(12)` → `StateProduce(HumanFireworks, 15)` |
| | Condition: `MoodBelowOrEq(12)` → `StateProduce(Perception, 10)` |
| 思衡托 | State: `Consume(HumanFireworks, floor(value/3))` → 写入房间效率 |

### 凯尔希·思衡托办公室挂件规则（当前设计口径）

凯尔希·思衡托的办公室技能要求同班存在指定精英干员：迷迭香、煌、逻各斯、烛煌、电弧、真言、机械师。每个实际不同的设施房间中只要有至少一名指定精英干员，该房间计 1 个精英设施；同一房间内的多名指定干员不重复计数，最多计 5 个房间。凯尔希·思衡托与这些挂件只有在同一班次实际进驻时，才读取对应房间数量并获得额外效率。

使用建议：将“凯尔希·思衡托办公室 + 指定精英挂件”作为一个 late competitive package。先完成生产、中枢和其他 required 体系，再从真实剩余空位中放置挂件；不得为凑挂件抢占更高收益岗位。候选比较应计入挂件原本设施的机会成本，只有完整 package 的全域结果优于不启用凯尔希·思衡托时才启用。由于挂件与凯尔希·思衡托必须同班，该 package 需要生成同班约束；不同班次可以重新选择挂件组合。

上述挂件选择与按不同设施计数的排班接入属于后续实现边界；当前办公室通用候选已能结算思衡托自身技能，但尚未以该 package 规则自动为她配套挂件。

### 跨体系挂件宿主裁决

挂件宿主按“技能合法性”和“策略备选”分开处理：角色已有的加工站、会客室或训练室技能，均保留为对应设施的合法挂件备选；技能角色在该设施的候选排序中优先于无技能角色。所有角色均可作为加工站和训练室的策略性挂件备选，训练室固定保留 1 个挂件位。

宿舍是通用休息 / 恢复宿主，除赫德雷体系中的伊内丝、W 外，其余挂件角色均可进入宿舍备选。伊内丝和 W 作为赫德雷挂件时不得进入宿舍，必须使用会客室、加工站或训练室等其他合法宿主；这条限制只作用于“作为赫德雷挂件”的关系，不改变她们作为普通角色或其他体系支持位的独立设施资格。

当前挂件宿主容量上限按班次策略预留：宿舍 4 人、加工站 1 人、会客室 1 人、训练室 1 人。容量只表示挂件候选上限，实际仍需扣除必休人员和已占用的高价值岗位。挂件收益按相对主角色最佳替代方案的超额效率计算，而不是把主角色触发后的完整效率全部归给挂件；挂件与主角色按同一班次 presence 绑定，不跨班继承收益。

## 4.8 黑键（宿舍 → 感知 → 无声共鸣）

**简化假设**：`DEFAULT_DORM_OCCUPANT_COUNT` = 20。

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 乐感 | `DormOccupantCount` → `StateProduce(Perception, ×1)` → `StateConvert(Perception→SilentEcho, 1:1)` |
| Tier0 | 徘徊旋律 | `StateConsumeToEff(SilentEcho, div=4)` |
| TierUp | 怅惘和声 | `StateConsumeToEff(SilentEcho, div=2)` |

243c 基准：精0 **+5%**（20÷4），精2 **+10%**（20÷2）。

## 4.9 乌有（宿舍 → 人间烟火 → 贸易%）

| Tier | 技能 | EffectAtom |
|------|------|------------|
| TierUp | 愿者上钩 | `DormOccupantCount` → `StateProduce(HumanFireworks, ×1)` → `StateConsumeToEff(HumanFireworks, div=1)` |

宿舍 20 人 → **+20%** 订单获取效率。

## 4.10 铎铃（人间烟火 → 心情消耗）

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 跋山涉水 | `MoodDrainDelta(-0.1)` + `MoodDrainPerStateStep(HumanFireworks, step=10, -0.01)` |
| TierUp | 万里传书 | `MoodDrainDelta(-0.1)` + `MoodDrainPerStateStep(HumanFireworks, step=10, -0.02)` |

精0 铎铃同房心情 **-0.12**，精2 **-0.14**。

## 4.11 泰拉大陆调查团（木天蓼 → 贸易% / 制造%）

**Producer**（中枢，≠ 三星黑角/夜刀）：火龙S黑角 + 麒麟R夜刀 → `layout.global.Matatabi`。

| 设施 | 技能 | EffectAtom |
|------|------|------------|
| 贸易 | 可爱的艾露猫 | `AddFlatEff(5)` + `AddLimitDelta(2)` + `StateConsumeToEff(Matatabi, div=1, mult=3)` |
| 制造 | 可靠的随从们 | `AddLimitDelta(8)` + `AddFlatEff(5)` + `StateConsumeToEff(Matatabi, div=1)` |

木天蓼 12 时：贸易 **+41%**（5+36）、制造 **+17%**（5+12）。

## 4.12 火龙S黑角 / 麒麟R夜刀（怪猎中枢 · 木天蓼 producer + 精2 全局注入）

≠ 三星**黑角**/**夜刀**。tag：`cc.g.monhun`。

| 干员 | 技能 | EffectAtom |
|------|------|------------|
| 火龙S黑角 精0 | 团队合作 | `TaggedCountInControl(monhun)` → `StateProduce(Matatabi, ×2)` |
| 火龙S黑角 精2 | 秘传交涉术 | `PeerTagInRoom(monhun)` → `GlobalInjectTradeEff(7)` |
| 麒麟R夜刀 精0 | 耐力回复 | `StateProduce(Matatabi, 8)` + `MoodDrainDelta(+0.5, self)` |
| 麒麟R夜刀 精2 | 以身作则 | `PeerTagInRoom(monhun)` → `GlobalInjectManuEff(2)` |

双人同中枢精0：木天蓼 **12**。精2 且队友条件满足：全贸易 **+7%**、全制造 **+2%**。
