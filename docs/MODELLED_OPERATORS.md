# 已建模干员

> 从 [`EFFECT_ATOM_DESIGN.md`](EFFECT_ATOM_DESIGN.md) §4 抽出。每新增干员时追加到本文末尾。

---

## 4.1 但书

| Tier | 技能 | EffectAtom |
|------|------|------------|
| 通用 | 合同法 | Condition: `GoldDeliveryBelow(4)` → Action: `TagOrder("breach")` |
| Tier0 | 违约索赔·α | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(1)` |
| TierUp | 违约索赔·β | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(2)` |

L2 产量：`for_trade_level` 裁剪订单档位；违约链在 2/3 金订单上叠加 `DOCUS_SUB4_LMD_BONUS`（与工具人表 2/3 金 +1000 对齐，校准见 `UNIT_OUTPUT_ANCHORS.csv`）。

**排班约束**：但书**单走一站**（但书 + 订单效率工具人）。同房互斥：但书 ↔ 巫恋低语/龙舌兰投资/裁缝 α/β；可露希尔 ↔ 精二巫恋低语；但书 ↔ 可露希尔。

**L3 但书单走**（`gsl_docus_solo`）：`trade_pct = order_eff_pre_shortcut`，`gold_pct=55`（×1.55）。

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
| TierUp | 市井之道 | Step1: `OtherOpsSettledEff` → `ReduceLimit(floor(eff/10))` |
| | | Step2: `OrderCount` → `AddFlatEffFromSelector(×4.0)`，`phase=order_var` |

**精0 摊贩**为无灵知时的贸易常用态（`AddPerGapEff`，依赖 order_gap）。**精1+ 市井**默认不进通用贸易池；中枢**灵知 E2·精密计算**激活时注入市井孑，由 L1 搜索与喀兰队友自然上浮。

`OrderCount` 语义：有市井 buff 时稳态按 `final_order_limit` 计 per-order（满槽假设）；否则用输入 `order_count` 并 clamp 至上限。

与雪雉·天道酬勤：`Condition::TiandaoEffVarAllowed` — 同房仅有孑+雪雉且无第三方 settled 时，天道酬勤不生效（市井之道优先）；有第三方贡献时两者均生效。

L1 自然回归：`reg_ling_jie_yaxin_natural` 锁定中枢灵知 E2 + 贸易站精1+孑 / 银灰 / 琳琅诗怀雅 = 129%，且 `trade_shortcut=None`。拆法：银灰受精密计算后 5% + 琳琅 20% = 25%；孑按 18 单上限给 72%；琳琅按超出 10 单的 8 单给 32%；合计 129%。`gsl_ling_jie_yaxin` 保留在 `trade_shortcuts.json` 作为参考锚点，不参与 active L3 匹配。

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

**编排**：巫恋是贸易 core priority，不是固定裁缝β三人组。`trade_segments.roles.witch` 强制包含精二巫恋，并通过 `gsl_witch_*` 匹配龙舌兰精二 + 裁缝 β / α / 空白第三人等 fallback；`witch_long_beta` 只保留为历史锚点 / L3 分档，不再作为主路径 fixed registry 早占站。

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
