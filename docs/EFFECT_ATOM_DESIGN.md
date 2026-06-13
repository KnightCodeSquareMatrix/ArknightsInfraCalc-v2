# EffectAtom 设计文档

> 本文档记录 ArknightsInfraCalc 重建模的核心设计，以及 Cursor 逐干员梳理的工作方式。

---

## 一、核心原则

1. **游戏机制是唯一权威**。不凭空设计"通用引擎"，从具体干员倒推需要的 Selector/Action/Condition。
2. **声明式 + 平坦**。每个 BuffDef 由 Selector + Action 组合而成，JSON 不写表达式、不写 if/else。
3. **运行时零正则**。所有数值在数据准备阶段显式填入，`parse.rs` 最终删除。
4. **tier 切换技能**。精 0 / 精 1 / 精 2 走不同的 BuffDef 列表，通过 `PromotionTier` 自动选择。

---

## 二、EffectAtom 模型

一个技能由一个 `EffectAtom` 或一组 `EffectAtom` 描述：

```
EffectAtom {
    selector: Selector,      // 从哪取数
    action: Action,          // 做什么计算
    condition: Option<Condition>,  // 什么情况下触发
    tag: Option<String>,     // 可选标记，供后续 phase 引用/修改
    phase: Phase,            // 执行阶段
    phase_order: i32,        // 阶段内排序
}
```

同一个 BuffDef 可以有多个 EffectAtom，自由组合。

---

## 三、已确认的 Selector / Action / Condition

以下全部从实际干员机制倒推得出，不凭空设计。

### Selector（数据源）

| Selector | 含义 | 来源 |
|----------|------|------|
| `GoldDeliveryCount` | 订单里的赤金交付数量 | 但书 |
| `OtherOpsDirectEff` | 其他干员直接写在技能上的效率（不含衍生/叠加） | 孑 |
| `OtherOpsTotalEff` | 其他干员的总效率 | 通用 |
| `FinalOrderLimit` | 第一步算完后的最终订单上限 | 孑 |
| `OrderGap` | 当前订单数与订单上限的差额 | 孑精 0 |
| `Mood` | 干员心情值 | 凯尔希 |

### Action（计算行为）

| Action | 含义 | 来源 |
|--------|------|------|
| `AddFlatEff(value)` | 增加固定效率 | 可露希尔、孑 |
| `AddPerGapEff(rate)` | 每差 1 笔订单增加效率 | 孑精 0 |
| `TagOrder(tag)` | 订单分类标签；L1 只打 tag，L2 `order_mechanic` 解释（见下表） | 但书、可露希尔、裁缝、佩佩、U-Official |
| `AddGoldDelivery(n)` | 赤金交付数额外增加 | 但书 |
| `ReduceLimit(ceil(eff/N), min=M)` | 按效率减少订单上限 | 孑 |
| `StateProduce(key, amount)` | 向全局状态池写入 | 凯尔希 |
| `StateConsume(key, formula)` | 从全局状态池读取并计算 | 思衡托 |
| `PeerEffAbsorb(rate_per_peer)` | 同房他人 trade 效率归零；每人向自身 +rate%（0=只清零） | 巫恋 45、佩佩 0 |

**`TagOrder` 注册表**（贸易站已用 tag → L2 行为；完整常数见 §8.6）：

| tag | 干员/技能 | L2 效果摘要 |
|-----|-----------|-------------|
| `breach` | 但书·合同法 | 违约链：`AddGoldDelivery` + LMD 加成 |
| `closure_special` | 可露希尔·特别订单 | 固定 2:24 / 1200 / 2 金特别单 |
| `tailor_alpha` | 裁缝 α / 手工艺品 α / 鉴定师眼光 / 懂行 | 贵金属 peak 分布（α 档） |
| `tailor_beta` | 裁缝 β / 手工艺品 β / 鉴定师手段 | 贵金属 peak 分布（β 档） |
| `pepe_exclusive` | 佩佩·慧眼独到 | 特别独占单 4:30 / 1000 / 0 金；**不吃 trade%** |
| `eureka` | U-Official·天真的谈判者 | 赤金交付强制 2；等效 gold% |

违约索赔的 `AddGoldDelivery` 走 `Condition: OrderHasTag("breach")`，不另打 tag。

### Condition（触发条件）

| Condition | 含义 | 来源 |
|-----------|------|------|
| `GoldDeliveryBelow(n)` | 赤金交付数量 < n | 但书 |
| `OrderHasTag(tag)` | 订单带有某标签 | 但书 |
| `MoodAbove(n)` | 心情 > n | 凯尔希 |
| `MoodBelowOrEq(n)` | 心情 ≤ n | 凯尔希 |
| `PeerTagInRoom(tag)` | 同房存在带 `tag` 的**其他**干员（不含技能持有者） | 火龙S黑角精2、麒麟R夜刀精2 |

`StateConsumeToEff(key, div, multiplier?)`：`floor(state/div)×mult` 计入效率%（齐尔查克 `mult=1`；泰拉调查团贸易 `mult=3`）。

### Phase 执行顺序

| Phase | 含义 | 说明 |
|-------|------|------|
| `state_write` | 状态池写入 | 中枢/宿舍干员生产状态值 |
| `constant` | 固定效率/上限 | 最基础的加减 |
| `limit` | 订单上限修订 | 孑压上限等 |
| `order_var` | 订单数相关变量 | per-order/per-gap |
| `eff_var` | 效率相关变量 | 基于当前效率的衍生计算 |
| `peer_absorb` | 他人效率归零/吸收 | `PeerEffAbsorb`（巫恋/佩佩） |
| `order_mechanic` | 订单机制 | `TagOrder` / `AddGoldDelivery` 等改写订单类别与交付 |
| `global_inject` | 中枢注入 | 控制中枢 buff 注入贸易/制造站 |

---

## 四、已建模干员

### 4.1 但书

| Tier | 技能 | EffectAtom |
|------|------|------------|
| 通用 | 合同法 | Condition: `GoldDeliveryBelow(4)` → Action: `TagOrder("breach")` |
| Tier0 | 违约索赔·α | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(1)` |
| TierUp | 违约索赔·β | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(2)` |

L2 产量：`for_trade_level` 裁剪订单档位；违约链在 2/3 金订单上叠加 `DOCUS_SUB4_LMD_BONUS`（与工具人表 2/3 金 +1000 对齐，校准见 `UNIT_OUTPUT_ANCHORS.csv`）。

**排班约束（公孙长乐）**：但书**单走一站**（但书 + 订单效率工具人）。同房互斥（`trade_station_exclusive_violation`）：**但书** ↔ 巫恋低语/龙舌兰投资/裁缝 α/β；**可露希尔** ↔ 精二巫恋低语；**但书** ↔ 可露希尔。搜索与 `solve_trade` 剔除非法三人组。

**L3 但书单走**（`gsl_docus_solo`）：`trade_pct = order_eff_pre_shortcut`（纸面工具效率），`gold_pct=55`（×1.55）；搜索/轮换直接 `C(n,3)` 取最高分，不按档分桶。

L1 `phase_order`：同房机制结算仍 可露希尔 > 但书（与 L3 搜索无关）。

### 4.2 可露希尔

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 总工程师 | 心情恢复（中枢，不在模拟范围） |
| TierUp | 特别订单 | Action: `AddFlatEff(10.0)` + Action: `TagOrder("closure_special")` |

特别订单：2:24:00，交付 2 赤金，1200 龙门币。不视作违约订单。

### 4.3 孑

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 摊贩经济 | Action: `AddPerGapEff(4.0)` |
| TierUp | 市井之道 | Step1: Selector: `OtherOpsSettledEff` → Action: `ReduceLimit(ceil(eff/10), min=1)` |
| | | Step2: Selector: `OrderCount` → Action: `AddFlatEffFromSelector(×4.0)`，`phase=order_var` |

**精0 摊贩**为无灵知时的贸易常用态（`AddPerGapEff`，依赖 order_gap）。**精1+ 市井**需中枢 **灵知 E2·精密计算** 与喀兰队友（灵孑银崖）才为游戏正解；见 §4.3.1。

`OrderCount` 语义：有房内有市井 buff 时稳态按 `final_order_limit` 计 per-order（满槽假设）；否则用输入 `order_count` 并 clamp 至上限。

与雪雉·天道酬勤：`Condition::TiandaoEffVarAllowed` — 同房仅有孑+雪雉且无第三方 settled 时，天道酬勤不生效（市井之道优先）；有第三方贡献时两者均生效。

L3：`gsl_ling_jie_yaxin`（125% 工具人表锚）；L1 裸算约 132%，差值保留在 L3。

#### 4.3.1 灵知 · 精密计算（跨设施 → 贸易房）

| Tier | 技能 | EffectAtom |
|------|------|------------|
| TierUp | 精密计算 | `Action::GlobalInjectKarlanPrecision { eff_per_karlan: -15, limit_per_karlan: 6 }`，`phase=global_inject` |

控制域写入 `GlobalInjectManifest::karlan_precision`；贸易域 `TradeContext::seed_karlan_precision()` 在相位前对同房 **`cc.g.karlan` 干员**（银灰、崖心等；孑无 tag）写入 settled_eff / limit_contrib，使市井 `ReduceLimit` 读到被 debuff 后的 `other_ops_settled_eff`。

**非目标**：灵知 E0「幕后指挥」心情恢复（`control_mp_cost&faction[030]`）。

### 4.5 雪雉

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 天道酬勤·α | Condition: `TiandaoEffVarAllowed` → Selector: `PeerSettledEffSum` → `AddBucketEffFromSelector(5/5, cap 25)` |
| TierUp | 天道酬勤·β | 同上，cap 35 |

### 4.7 巫恋

| Tier | 技能 | EffectAtom |
|------|------|------------|
| TierUp | 低语 | `PeerEffAbsorb(45)` + 全体 `MoodDrainDelta(+0.25)` |

与佩佩共用 `PeerEffAbsorb` 原语；巫恋 `rate_per_peer=45`，佩佩 `rate=0`（只清零、不吸收）。

### 4.6 佩佩

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 多面逢源 | Selector: `FacilityLevel` → `AddLimitFromSelector(×1)` |
| TierUp | 慧眼独到 | `PeerEffAbsorb(0)` + `TagOrder("pepe_exclusive")` |

**L2 特别独占订单**（`SpecialOrderKind::PepeExclusive`）：4:30:00，0 赤金，1000 龙门币；不视作违约；**不受任何订单获取效率影响**（L2 score / 日产量均不乘纸面 trade%）。同房效率工具人由 L1 清零；搜索仍用 `pepe_station_trade_eff_violation` 剔除「佩佩+效率人」非法组合。

### 4.4 凯尔希 / 思衡托（跨房间）

| 干员 | EffectAtom |
|------|------------|
| 凯尔希 | Condition: `MoodAbove(12)` → `StateProduce(HumanFireworks, 15)` |
| | Condition: `MoodBelowOrEq(12)` → `StateProduce(Perception, 10)` |
| 思衡托 | State: `Consume(HumanFireworks, floor(value/3))` → 写入房间效率 |

### 4.8 黑键（宿舍 → 感知 → 无声共鸣）

**简化假设**：`TradeLayoutContext.dorm_occupant_count` 默认 **20**（全基建宿舍恒满员）；未在蓝图/assignment 中覆盖时同上。

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 乐感 | Selector: `DormOccupantCount` → `StateProduce(Perception, ×1)` → `StateConvert(Perception→SilentEcho, 1:1)` |
| Tier0 | 徘徊旋律 | `StateConsumeToEff(SilentEcho, div=4)` |
| TierUp | 怅惘和声 | `StateConsumeToEff(SilentEcho, div=2)` |

243c 基准（宿舍 20 人）：精0 **+5%**（20÷4），精2 **+10%**（20÷2）。乐感转化作用于同房 `state_pool` 内全部感知（含 `layout.global` 快照）。

### 4.9 乌有（宿舍 → 人间烟火 → 贸易%）

精2 才有贸易绑定（`trade_ord_spd_bd_n2[000]`）。

| Tier | 技能 | EffectAtom |
|------|------|------------|
| TierUp | 愿者上钩 | Selector: `DormOccupantCount` → `StateProduce(HumanFireworks, ×1)` → `StateConsumeToEff(HumanFireworks, div=1)` |

宿舍 20 人 → **+20%** 订单获取效率。

**跨站人间烟火**：`resolve_base` 在乌有进驻任意贸易站时，向 `layout.global` 注入 `dorm_occupant_count` 点人间烟火（供铎铃等 consumer）；乌有所在贸易站的 per-room layout 会扣回等量注入，避免同房 state_write 重复计数。

### 4.10 铎铃（人间烟火 → 心情消耗）

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 跋山涉水 | `MoodDrainDelta(-0.1)` + `MoodDrainPerStateStep(HumanFireworks, step=10, -0.01)` |
| TierUp | 万里传书 | `MoodDrainDelta(-0.1)` + `MoodDrainPerStateStep(HumanFireworks, step=10, -0.02)` |

读取同房 `state_pool` 中人间烟火（来自 `layout.global` 快照或 `TradeRoomInput.human_fireworks`）。乌有在别站上岗且宿舍满 20 人时，精0 铎铃同房心情 **-0.12**（-0.1 - 2×0.01），精2 **-0.14**（-0.1 - 2×0.02）。

### 4.11 泰拉大陆调查团（木天蓼 → 贸易% / 制造%）

**Producer**（中枢，≠ 三星黑角/夜刀）：**火龙S黑角** `团队合作` + **麒麟R夜刀** `耐力回复` → `layout.global.Matatabi`。怪猎双人同中枢基准 **12** 点（见 §4.12）。

| 设施 | 技能 | buff_id | EffectAtom |
|------|------|---------|------------|
| 贸易 | 可爱的艾露猫 | `trade_ord_spd&limit&bd[000]` | `AddFlatEff(5)` + `AddLimitDelta(2)` + `StateConsumeToEff(Matatabi, div=1, mult=3)` |
| 制造 | 可靠的随从们 | `manu_prod_spd&limit&bd[000]` | `AddLimitDelta(8)` + `AddFlatEff(5)` + `StateConsumeToEff(Matatabi, div=1)` |

木天蓼 12 时：贸易 **+41%**（5+36）、制造 **+17%**（5+12）。布局：`snhunt_baseline()`；精2 满配另叠 §4.12 全站 +7% / +2%。

### 4.12 火龙S黑角 / 麒麟R夜刀（怪猎中枢 · 木天蓼 producer + 精2 全局注入）

≠ 三星 **黑角** / **夜刀**。tag：`cc.g.monhun`（与贸易 `cc.g.snhunt` 的焰狐龙梓兰等分开；**梓兰不计入**木天蓼团队合作计数）。

| 干员 | 技能 | EffectAtom |
|------|------|------------|
| 火龙S黑角 精0 | 团队合作 | `TaggedCountInControl(monhun)` → `StateProduce(Matatabi, ×2)` |
| 火龙S黑角 精2 | 秘传交涉术 | `PeerTagInRoom(monhun)` → `GlobalInjectTradeEff(7)`，`trade_global_flat` 族 |
| 麒麟R夜刀 精0 | 耐力回复 | `StateProduce(Matatabi, 8)` + `MoodDrainDelta(+0.5, self)` |
| 麒麟R夜刀 精2 | 以身作则 | `PeerTagInRoom(monhun)` → `GlobalInjectManuEff(2)`，`manu_global_all` 族 |

双人同中枢精0：木天蓼 **12**。精2 且队友条件满足：全贸易 **+7%**、全制造 **+2%**（与阿米娅/凯尔希同族取最高）。布局：`snhunt_baseline()` / `snhunt_elite2_baseline()`；编制见 `snhunt_default_assignment()` / `snhunt_control_assignment(elite)`。

---

## 五、Cursor 逐干员梳理流程

每分析一个干员时，遵循以下步骤：

### 步骤 1：读取原始技能文本

从 `prts_trade_skills.json`（PRTS 贸易站 §5 表格快照）读取该干员技能的原始机制描述。

### 步骤 2：判断 tier 切换

该干员精 0 和精 1/2 的技能是否有差异？如果技能在 `tier_0` 和 `tier_up` 不同，需要分别建模。

### 步骤 3：识别 EffectAtom

对每个技能，回答三件事：

1. **读什么**？（Selector — 从哪取数据）
2. **算什么**？（Action — 对数据做什么）
3. **什么条件下触发**？（Condition — 前置条件）

### 步骤 4：判断是否需要新增 Selector/Action/Condition

如果现有的 Selector / Action / Condition 无法覆盖，记录为新候选，等待后续干员确认是否会重复使用。

### 步骤 4b：选层（积木 / 域短路 / 组合短路）

见第八节。提案中须标明 L1 only、L1+L2（`gold_flow` / `order_mechanic`）、或 L3（`trade_shortcuts.json`）。

### 步骤 5：记录

将结果追加到本文档的"已建模干员"章节。

### 示例格式

```
### 银灰

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 喀兰贸易·α | Selector: TaggedInRoom("cc.g.karlan") → Action: AddFlatEff(tagged_count × 15.0) |
| TierUp | 喀兰贸易·β | Selector: TaggedInRoom("cc.g.karlan") → Action: AddFlatEff(tagged_count × 20.0) |

需要新增：Selector `TaggedInRoom(tag)`、Action 已有。
```

---

## 六、四层不变式（定稿）

1. **`skill_table.id` = 解包 `buff_id`**（禁止 `skill_*` 人工别名）
2. **干员归属只在 `operator_instances`**：`干员@tier_0` / `干员@tier_up` → `buff_ids` + `stepwise`
3. **`resolve_buff_ids`**：`stepwise=true` 且 `tier_up` 时合并 tier_0；同 stem 的 buff 由 tier_up 变体替换
4. **游戏原文只信 `prts_trade_skills.json`**；建模核对走 PRTS → instances → skill_table

| 层 | 文件 | 职责 |
|----|------|------|
| 原文 | `prts_trade_skills.json` | 技能名 + 描述 + 持有人 |
| 机制 | `skill_table.json` | `buff_id` → `EffectAtom[]`（含**委托标记**，见第八节） |
| 干员 | `operator_instances.json` | 晋升状态 → 解析后的 `buff_ids` |
| 运行时 | 见第八节 | 积木解释器 + 域短路 + 组合短路 |

---

## 八、分层求解：积木 vs 域短路（定稿）

v2 **不是**「只有一个 interpreter」。贸易站求解是**三层协作**，域短路与组合短路是**刻意设计的最优路径**，不是待还的技术债，也**不要求**强行迁回 JSON 积木。

### 8.1 三层架构

```
prts / MECHANICS_REGISTRY（原文）
        ↓
skill_table.json + operator_instances.json（注册与归属）
        ↓
┌─────────────────────────────────────────────────────────────┐
│ L1 主路径：interpreter                                       │
│     Phase 排序 → Selector / Condition / Action               │
│     平坦、可声明、可组合（Scratch 积木）                      │
├─────────────────────────────────────────────────────────────┤
│ L2 域短路：机制域专用引擎                                     │
│     gold_flow      — 赤金生产线链（进驻顺序 × 虚拟线状态机）   │
│     order_mechanic — 订单类型/分布 → 等效贸易效率             │
├─────────────────────────────────────────────────────────────┤
│ L3 组合短路：shortcut + trade_shortcuts.json                 │
│     巫恋工具人表、可露希尔分档等 — 组合分类 → 表化最优解       │
└─────────────────────────────────────────────────────────────┘
        ↓
solve_trade → effective_eff_multiplier
```

**调用顺序**（`solver.rs`）：先跑 L1（含 L2 在 phase 管线中的挂钩点）→ 若 L3 匹配则覆盖最终 `trade_pct` / 等效乘子 → 否则用 L2 `order_mechanic` 算机制等效。

### 8.2 何时用哪一层？

| 判定 | 用 L1 EffectAtom | 用 L2 域短路 | 用 L3 组合短路 |
|------|------------------|--------------|----------------|
| 单 buff、按 phase 可排序 | ✅ 默认 | | |
| 读 selector、写 settled_eff / limit，无跨步可变共享状态 | ✅ | | |
| **同房间进驻顺序**影响后续 buff 的输入（虚拟赤金线累加） | | ✅ `gold_flow` | |
| 订单抽样/分布/违约链的**等效效率**（非单笔 flat%） | 部分用 `order_mechanic` phase 打 tag | ✅ `order_mechanic` | |
| 组合结果已**表化**且为搜索/回归热路径（公孙长乐基准站） | 仍保留 L1 算 `order_eff_pre` | | ✅ `shortcut` |
| 用 JSON 表达会引入**假原子**或重复状态机 | 不要硬塞 | ✅ 域引擎 | |

**默认规则**：新技能先尝试 L1；若步骤 3 识别出「顺序迭代共享变量」或「分布等效」，在提案中**显式标注**走 L2/L3，而不是把逻辑散落成干员名分支。

### 8.3 L1 约束（仍然有效）

- 代码**不认识干员名**；`interpreter` 只认 `buff_id`、Selector、Condition、Action。
- `skill_table` 不写表达式、不写 if/else；数值在数据准备阶段填好。
- 干员归属、tag 只在 `operator_instances` / `data/tags/`。

L2/L3 **允许**按 `buff_id` 或**组合分类**分支，因为这是**机制 id / 组合类型**，不是「if 深巡 then …」式补丁。

### 8.4 委托标记：`skill_table` 空 atoms

下列 buff 在 `skill_table.json` 中 **`atoms: []` 是刻意的**，表示「已注册、已绑定干员，执行权委托给域引擎」：

| buff_id | 委托给 | 说明 |
|---------|--------|------|
| `trade_ord_line_gold[000/010]` | `gold_flow` | 绮良：按真实线数追加虚拟线 |
| `trade_ord_line_durin[010]` | `gold_flow` | 鸿雪：杜林虚拟线 |
| `trade_ord_spd&gold[000/010/100]` | `gold_flow` | 图耶/鸿雪：按总线数追加效率% |

**不要**把空 atoms 当成「未建模」；`check_trade_roster` 报告中的「空 atoms」是**委托清单**，不是缺失清单。

新增域短路时：在 `skill_table` 保留 buff 行（可空 atoms）+ 在对应引擎注册 `buff_id` 分支 + 补 `gold_flow` / `order_mechanic` 测试。

### 8.5 L2：`gold_flow`（赤金链）

- **触发点**：`interpreter` 在 `PeerAbsorb` 之前调用 `apply_gold_flow_chain`。
- **输入**：`TradeRoomInput::gold_production_lines`（真实线）、`durin_virtual_lines`（布局/杜林人数）。
- **状态**：`virtual_gold_lines` 随进驻顺序递增；后位干员读到的是**前面累加后的总线数**。
- **为何不走 L1**：链式机制本质是**有序状态机**；拆成多条 Atom 会在同一 phase 内重复读写共享变量，比单遍扫描更慢、更难测。

### 8.6 L2：`order_mechanic`（订单域）

- **输入**：L1 产出的 `order_tags`、`law_active`、`order_lmd_bonus` 等。
- **输出**：`mechanic_equiv_eff_pct`（违约/裁缝/可露希尔/龙舌兰/eureka 等的等效加成）。
- **与 L1 分工**：L1 的 `order_mechanic` **phase** 只负责打 tag / 替换订单类型；**分布与等效效率**由 `order_mechanic.rs` 统一算。

**特殊订单常数表**（L2 权威；L1 仅 `TagOrder`，见 §三 `TagOrder` 注册表）：

| tag | 时长 | 龙门币 | 赤金 | 吃 trade%？ | 干员 |
|-----|------|--------|------|-------------|------|
| `closure_special` | 2:24:00 | 1200 | 2 | 是 | 可露希尔 |
| `eureka` | 2:24:00（强制 2 金档） | 常规 | 2 | 部分（等效 gold%） | U-Official |
| `pepe_exclusive` | 4:30:00 | 1000 | 0 | **否** | 佩佩 |

**贸易站 `PeerEffAbsorb` 用法**：巫恋 45（清零+吸收）；佩佩 0（清零）。制造站「他人生产力归零」（冬时/森蚺等）为别设施域，复用同一语义、另开 manu 引擎时再落。

### 8.7 L3：`shortcut`（组合表）

- **数据**：`data/trade_shortcuts.json`（`trade_pct`、`gold_pct`、可选 `match` 规则）。
- **逻辑**：`shortcut.rs` — 但书 `gsl_docus_solo`（动态 trade%）/ 巫恋组 / 可露希尔分档；非法同房不进 L3。轮换贪心逐站 `search_trade_triples` 取最优。
- **角色**：
  1. **搜索/穷举热路径的最优解查表**（不必每次完整展开订单蒙特卡洛）；
  2. **`infra-cli verify` 回归锚点**（防止 L1+L2 漂移）。
- L3 命中时仍保留 `order_eff_pre_shortcut` 字段，便于对照 L1 积木是否算对。

### 8.8 协作流程补充（第五节步骤 4 之后）

识别 EffectAtom 后，增加一步 **「选层」**：

1. 仅 L1 → 写满 `atoms`，补 interpreter 测试。
2. L1 + L2 → L1 写 tag/常数部分；链式或分布部分写引擎 + 委托空 atoms（若适用）。
3. L1 + L2 + L3 → 在 `trade_shortcuts.json` 增加或更新条目，`verify --all` 必过。

**禁止**：为省事在 `solver` / `pool` / `search` 里写干员名特例；应扩 Selector/Action，或扩 L2/L3 的 `buff_id`/组合分类。

### 8.9 与全基建扩展的关系

`MECHANICS_REGISTRY.csv`（727 条）中，制造/宿舍/中枢等设施**复用同一 EffectAtom 词汇**，但可有各自的**域短路**（例：制造生产力链、宿舍状态写入）。每设施落地时重复本节判定，而不是假设「全部 727 条都必须是 JSON 积木」。

### 8.10 排班层（贸易站三班 A-B-A）

三班轮换属于**场景编排**，不引入新 EffectAtom，也不改写 L1 解释器。

| 假设 | 说明 |
|------|------|
| 范围 | 仅贸易站；每班 3 站 × 3 人 |
| 心情 | 固定 `mood = 24`（效率上界基准）；心情排班见 §8.12 |
| 池修剪 | 第 n 班在 `operbox.owned \ 上一班 9 人` 上建池（只排除相邻班） |
| A-B-A | 第 1 班搜索；第 2 班修剪后重搜；第 3 班拷贝第 1 班并校验与第 2 班不交 |
| 布局 | 三班共用 `TradeLayoutContext::search_baseline()`（或外置 JSON，另开任务） |
| 单班算法 | 贪心逐站 `search_trade_triples` 取得分最高三人组 |
| 但书 | 独占一站（+ 两订单效率人）；与裁缝组不同站；见 §4.1 |

入口：`schedule_trade_rotation_a_b_a`（`infra-core`）、`infra-cli schedule rotation --operbox …`。

### 8.11 产量场景层（单位产出 / 无人机 / 上班）

不属于 EffectAtom；由 L2 分布 + 纸面效率% 组合。

| 概念 | 实现 |
|------|------|
| 贸易站等级分布 | `GoldDistribution::for_trade_level`（1 级仅 2 金，2 级无 4 金） |
| 但书 2/3 金 +LMD | `DOCUS_SUB4_LMD_BONUS`（`order_mechanic.rs`） |
| 单位贸易/赤金产出 | `trade/unit_output.rs`：`1440 × Σ p·(lmd/dur)` |
| 倍率 | `multiplier_vs_lv3_regular` |
| 日产量 | `daily_yield`：`纸面eff% × (shift/24) × unit` |
| 无人机 | `DRONE_TRADE_FACTOR = 0.685` |
| 工具人表赤金显示 | `gsl_unit_gold()` = 内部 × `GSL_GOLD_UNIT_SCALE(100)` |
| 锚点回归 | `data/UNIT_OUTPUT_ANCHORS.csv` + `verify --all` |
| CLI | `infra-cli trade yield <fixture> [--level] [--shift]` |

**边界**：不建模制造站产金平衡；但书与裁缝组仍互斥（§4.1）；L3 shortcut 的 `trade_pct` 与 L2 单位产出可并存对照（`order_eff_pre_shortcut`）。

### 8.12 求解器职责边界（非目标：心情排班）

本仓库的贸易站求解器回答一个问题：

> **给定 operbox（及场景参数），哪种同房组合效率最高？**

它输出的是**效率排序与机制求值**（score、shortcut、单位产出），不是「基地明天能不能真这么连班排下去」。

| 在本求解器 | 在更宏观的规划器（另项目/上层） |
|------------|--------------------------------|
| `operbox` → 贸易池 → 同房 `C(n,3)` 搜索 | 全基建岗位分配（贸易/制造/宿舍/中枢） |
| L1/L2/L3 机制求值与公孙锚点对齐 | 心情预算、宿管恢复、8h 上班与连班可行性 |
| A-B-A 轮换：相邻班人不重叠下的**效率贪心** | 「核心核能否三班都开」「第 2 班是否该留高耗心情人」 |
| `TradeLayoutContext` 等**场景假设**（制造线数、伺夜在岗等） | 从真实基建状态生成并注入上述假设 |
| **宿舍进驻人数恒 20**（`DEFAULT_DORM_OCCUPANT_COUNT`；黑键/乌有状态链） | 真实宿舍排班与进驻人数 |
| **怪猎基准布局**（`snhunt.json` + 中枢双人；木天蓼 12；精2 另 +7% 贸易 / +2% 制造） | 真实中枢编制与令/夕等烟火 producer |
| 固定 `mood = 24` 作为**效率上界基准** | `MoodTrack`、逐小时心情与换班决策 |

**心情的两层含义（勿混淆）**：

1. **L1 机制求值**：技能上的 `MoodAbove` / `MoodBelowOrEq`、`mood_drain_delta` 等仍属 EffectAtom 范畴——算的是「若心情为 X，该 buff 是否触发、每小时耗多少」。
2. **排班约束**：搜索与轮换**始终**传入 `mood = 24`，不把心情当作硬约束，也不建模宿管/恢复。这不是待补缺口，而是**刻意非目标**。

上层规划器可多次调用本求解器（不同干员子集、不同 `layout`、是否允许开机制核），用返回的 score / 产量做输入，再自己做心情可行性与最终 timetable。

**全基建单班进驻编制**（并行搜 + 全局 `used` 落位 → `BaseAssignment`）的设计定稿见 **[BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md)**；与 §8.10 贸易三班轮换正交。实现待落地。

**禁止**：为「排班更真」在本仓库的 `search` / `schedule` 中接入心情追踪、宿管编制或跨设施恢复链；应在上层规划器消费本求解器的效率输出。

---

## 九、待解决

- [x] 孑 / 雪雉特殊叠加 — `TiandaoEffVarAllowed` + `PeerSettledEffSum` / `OrderCount` / `OtherOpsSettledEff`
- [x] **灵知精密计算 → 贸易房喀兰注入** — `GlobalInjectKarlanPrecision` + `seed_karlan_precision`；L3 `gsl_ling_jie_yaxin`
- [~] **市井之道产能耦合** — OrderCount 稳态/limit clamp 已修；L1 纸面 vs 工具人表 125% 仍靠 L3；`unit_output` 未全解 limit→吞吐；精0 摊贩仍为无灵知默认态
- [ ] 确认完整的 Phase 列表和排序
- [ ] `StateProduces` 的 formula 表达能力（`dorm_total`、`extra_hire` 等）— 跨房间状态用 layout 注入 vs 全基建 producer
- [ ] `trade_search_layout` 外置 JSON（场景假设与 L1 机制分离）
- [x] **贸易站收尾：佩佩** — `PepeExclusive` + `unit_output` / score 不受纸面 trade% 影响
- [x] **`GlobalResourceKey` 词汇表** — `crates/infra-core/src/global_resource/`（16 种资源 + `REGISTRY` / `CONVERSIONS`）
- [x] **P0 木天蓼链** — 火龙S黑角/麒麟R夜刀 producer + 泰拉调查团 consumer + `snhunt_baseline`（§4.11–4.12）
- [x] **P0 宿舍人数链（简化）** — 黑键/乌有/铎铃 + `DEFAULT_DORM_OCCUPANT_COUNT=20`（§4.8–4.10）
- [ ] **全局资源 producer 全量** — 令/夕/重岳烟火、森西料理、宿舍感知链等；发电站/办公室尚未系统化

### 8.13 全局资源注册表

**布局（Blueprint）** 只描述物理设施；下列资源均由 [`GlobalResourcePool`](../../crates/infra-core/src/global_resource/pool.rs) 统一管理。代码真相源：`global_resource/registry.rs` 的 `REGISTRY` / `CONVERSIONS`。

| `GlobalResourceKey` | 中文 | 典型 producer | 典型 consumer | 阶段 |
|---------------------|------|---------------|---------------|------|
| `Matatabi` | 木天蓼 | 中枢·**火龙S黑角** / **麒麟R夜刀**（≠ 三星黑角、夜刀） | 泰拉大陆调查团 | P0 |
| `Perception` | 感知信息 | 令/夕/黑键/迷迭香/梦境链 | →无声共鸣、→思维链环 | P0 |
| `VirtualPower` | 虚拟发电站 | 森蚺、承曦晨曦 | `PowerStationCount` | P0 |
| `VirtualGoldLines` | 虚拟赤金产线 | 鸿雪、绮良/图耶 | 贸易%、`gold_flow` | P0 |
| `HumanFireworks` | 人间烟火 | 令/夕/重岳/桑葚/乌有 | 铎铃、截云、黍、余 | P0 |
| `SilentEcho` | 无声共鸣 | 塑心、深律、黑键转化 | 黑键贸易% | P0 |
| `MonsterCuisine` | 魔物料理 | 森西宿舍 | 齐尔查克、玛露西尔 | P0 |
| `Dream` | 梦境 | 爱丽丝宿舍 | →感知（梦境呓语） | P0 |
| `MusicalSection` | 小节 | 车尔尼宿舍 | →感知（琴键漫步） | P0 |
| `MemoryFragment` | 记忆碎片 | 絮雨办公室 | →感知（追忆，耗尽清空） | P0 |
| `WitchcraftCrystal` | 巫术结晶 | 截云（5烟火→1） | 截云制造% | P0 |
| `ThoughtChainRing` | 思维链环 | 迷迭香超感 | 迷迭香制造% | P0 |
| `IntelligenceReserve` | 情报储备 | 灰烬中枢 | 闪击/霜华/双月 | P1 |
| `UsautDrink` | 乌萨斯特饮 | 战车中枢 | 导火索、闪击、霜华 | P1 |
| `Passion` | 热情值 | 初华/祥子体系中枢 | 祥子制造%、睦贸易% | P1 |
| `EngineeringRobot` | 工程机器人 | 至简（全图扫描） | 至简机械辅助 | P2 |

**已知转化边**（`CONVERSIONS`）：梦境/小节/记忆碎片 →感知 1:1；感知→无声共鸣 1:1；感知→思维链环 1:1；人间烟火→巫术结晶 5:1。

**不进全局池**：武道（训练室局部）、因果/业报（加工站局部）、心情、物理设施数、招募位布局数。
