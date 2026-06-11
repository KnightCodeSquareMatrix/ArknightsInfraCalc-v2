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
| `TagOrder(tag)` | 给当前订单打标签 | 但书 |
| `AddGoldDelivery(n)` | 赤金交付数额外增加 | 但书 |
| `ReplaceOrder(type)` | 替换为指定订单类型 | 可露希尔 |
| `ReduceLimit(ceil(eff/N), min=M)` | 按效率减少订单上限 | 孑 |
| `StateProduce(key, amount)` | 向全局状态池写入 | 凯尔希 |
| `StateConsume(key, formula)` | 从全局状态池读取并计算 | 思衡托 |

### Condition（触发条件）

| Condition | 含义 | 来源 |
|-----------|------|------|
| `GoldDeliveryBelow(n)` | 赤金交付数量 < n | 但书 |
| `OrderHasTag(tag)` | 订单带有某标签 | 但书 |
| `MoodAbove(n)` | 心情 > n | 凯尔希 |
| `MoodBelowOrEq(n)` | 心情 ≤ n | 凯尔希 |

### Phase 执行顺序

| Phase | 含义 | 说明 |
|-------|------|------|
| `state_write` | 状态池写入 | 中枢/宿舍干员生产状态值 |
| `constant` | 固定效率/上限 | 最基础的加减 |
| `limit` | 订单上限修订 | 孑压上限等 |
| `order_var` | 订单数相关变量 | per-order/per-gap |
| `eff_var` | 效率相关变量 | 基于当前效率的衍生计算 |
| `order_mechanic` | 订单替换 | 但书、可露希尔替换订单类型 |
| `global_inject` | 中枢注入 | 控制中枢 buff 注入贸易/制造站 |

---

## 四、已建模干员

### 4.1 但书

| Tier | 技能 | EffectAtom |
|------|------|------------|
| 通用 | 合同法 | Condition: `GoldDeliveryBelow(4)` → Action: `TagOrder("breach")` |
| Tier0 | 违约索赔·α | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(1)` |
| TierUp | 违约索赔·β | Condition: `OrderHasTag("breach")` → Action: `AddGoldDelivery(2)` |

优先级：可露希尔 > 但书（phase_order 保证顺序）。

### 4.2 可露希尔

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 总工程师 | 心情恢复（中枢，不在模拟范围） |
| TierUp | 特别订单 | Action: `AddFlatEff(10.0)` + Action: `ReplaceOrder("closure_special")` |

特别订单：2:24:00，交付 2 赤金，1200 龙门币。不视作违约订单。

### 4.3 孑

| Tier | 技能 | EffectAtom |
|------|------|------------|
| Tier0 | 摊贩经济 | Action: `AddPerGapEff(4.0)` |
| TierUp | 市井之道 | Step1: Selector: `OtherOpsDirectEff` → Action: `ReduceLimit(ceil(eff/10), min=1)` |
| | | Step2: Selector: `FinalOrderLimit` → Action: `AddFlatEff(L × 4.0)` |

特殊叠加规则：天道酬勤单独存在→孑优先。天道酬勤+其他技能→孑叠在上面。**可能需要代码处理。**

### 4.4 凯尔希 / 思衡托（跨房间）

| 干员 | EffectAtom |
|------|------------|
| 凯尔希 | Condition: `MoodAbove(12)` → `StateProduce(HumanFireworks, 15)` |
| | Condition: `MoodBelowOrEq(12)` → `StateProduce(Perception, 10)` |
| 思衡托 | State: `Consume(HumanFireworks, floor(value/3))` → 写入房间效率 |

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

- **输入**：L1 产出的 `order_tags`、`replace_order`、`law_active`、`order_lmd_bonus` 等。
- **输出**：`mechanic_equiv_eff_pct`（违约/裁缝/可露希尔/龙舌兰/eureka 等的等效加成）。
- **与 L1 分工**：L1 的 `order_mechanic` **phase** 只负责打 tag / 替换订单类型；**分布与等效效率**由 `order_mechanic.rs` 统一算。

### 8.7 L3：`shortcut`（组合表）

- **数据**：`data/trade_shortcuts.json`（`trade_pct`、`gold_pct`、可选 `match` 规则）。
- **逻辑**：`shortcut.rs` — 巫恋组分类优先，其次可露希尔 `order_eff_pre` 分档。
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

---

## 九、待解决

- [ ] 孑的特殊叠加规则（天道酬勤）— 可能需 L2 叠加顺序或 L1 `AdjustTagged`
- [ ] `AdjustTagged` — 回溯修改标记值，孑是否需要
- [ ] 确认完整的 Phase 列表和排序
- [ ] `StateProduces` 的 formula 表达能力（`dorm_total`、`extra_hire` 等）— 跨房间状态用 layout 注入 vs 全基建 producer
- [ ] `trade_search_layout` 外置 JSON（场景假设与 L1 机制分离）
