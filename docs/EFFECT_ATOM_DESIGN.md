# EffectAtom 设计文档

> 本文档记录 ArknightsInfraCalc 重建模的核心设计。
> **已建模干员详情**见 [`MODELLED_OPERATORS.md`](MODELLED_OPERATORS.md)。
> **体系链**见 [`SYSTEM_CHAINS.md`](SYSTEM_CHAINS.md)。
> **改机制协作流程**见 [`../AGENTS.md`](../AGENTS.md) §5；准备实现事项见 [`TODO/`](TODO/)。

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
| `OtherOpsSettledEff` | 同房他人在 settled_eff 上的值（含 PeerAbsorb/压缩后） | 孑市井、雪雉 |
| `OrderCount` | 当前实际订单数 | 孑市井 |
| `PeerSettledEffSum` | 同房他人 settled_eff 之和 | 雪雉 |
| `DormOccupantCount` | 全局宿舍进驻人数 | 黑键、乌有 |
| `RoomPeerCount` | 同房除自己外人数 | 佩佩 |
| `RoomOperatorCount` | 同房总人数 | 通用 |
| `FacilityLevel` | 设施等级 | 佩佩 |
| `TradeStationCount` | 贸易站数量 | 清流（制造站读） |
| `PowerStationCount` | 电站数量（含虚拟） | 温蒂（制造站读） |
| `EliteFacilityCount` | 精英化设施数量 | 深律 |
| `SuiFacilityCount` | 岁/令同行设施数 | 黍 |
| `DormLevelSum` | 宿舍有效等级之和（`dorm_ambience_level`；旧布局兼容 `dorm_beds`） | 深律 |
| `MeetingMaxLevel` | 会客室最高等级 | 深律 |
| `LimitExcess` | 订单上限超出当前订单数 | 诗怀雅 |
| `TaggedCountInRoom(tag)` | 同房带某 tag 的人数 | 银灰 |
| `TaggedCountInControl(tag)` | 中枢带某 tag 的人数 | 火龙S黑角 |
| `LimitContribSum` | 同房 limit_contrib 之和 | 雪雉 |
| `ManuRecipeKinds` | 制造站配方类型数 | 淬羽赫默 |
| `StateValue(key)` | 全局状态池数值 | 凯尔希、思衡托 |
| `Mood` | 心情值 | 凯尔希 |

### Action（计算行为）

| Action | 含义 | 来源 |
|--------|------|------|
| `AddFlatEff(value)` | 增加固定效率 | 可露希尔、孑 |
| `AddPerGapEff(rate)` | 每差 1 笔订单增加效率 | 孑精 0 |
| `TagOrder(tag)` | 订单分类标签 | 但书、可露希尔等 |
| `AddGoldDelivery(n)` | 赤金交付数额外增加 | 但书 |
| `ReduceLimit(floor(eff/N))` | 按 selector 效率压缩订单上限；最终订单最少 1 由上限重算 clamp 保证 | 孑 |
| `StateProduce(key, amount)` | 向全局状态池写入 | 凯尔希 |
| `StateConsume(key, formula)` | 从全局状态池读取并计算 | 思衡托 |
| `PeerEffAbsorb(rate_per_peer)` | 同房他人效率归零；每人向自身 +rate% | 巫恋 45、佩佩 0 |
| `AddEffRamp(rate_per_hour, cap)` | 时间爬升效率 | 芬、克洛丝 |
| `StateConsumeToEff(key, div, multiplier?)` | 状态消费 → 效率 | 黑键、齐尔查克 |
| `MoodDrainDelta(delta)` | 改变心情消耗速率 | 铎铃、巫恋 |
| `AddLimitDelta(n)` | 订单上限增加 | 银灰、讯使 |
| `AddBucketEffFromSelector(rate, cap)` | 按 selector 值加 bucket 式效率 | 雪雉 |

**`TagOrder` 注册表**（贸易站已用 tag → L2 行为）：

| tag | 干员/技能 | L2 效果摘要 |
|-----|-----------|-------------|
| `breach` | 但书·合同法 | 违约链：`AddGoldDelivery` + LMD 加成 |
| `closure_special` | 可露希尔·特别订单 | 固定 2:24 / 1200 / 2 金特别单 |
| `tailor_alpha` | 裁缝 α / 手工艺品 α / 鉴定师眼光 / 懂行 | 贵金属 peak 分布（α 档） |
| `tailor_beta` | 裁缝 β / 手工艺品 β / 鉴定师手段 | 贵金属 peak 分布（β 档） |
| `pepe_exclusive` | 佩佩·慧眼独到 | 特别独占单 4:30 / 1000 / 0 金；**不吃 trade%** |
| `eureka` | U-Official·天真的谈判者 | 赤金交付强制 2；等效 gold% |

### Condition（触发条件）

| Condition | 含义 | 来源 |
|-----------|------|------|
| `GoldDeliveryBelow(n)` | 赤金交付数量 < n | 但书 |
| `OrderHasTag(tag)` | 订单带有某标签 | 但书 |
| `MoodAbove(n)` | 心情 > n | 凯尔希 |
| `MoodBelowOrEq(n)` | 心情 ≤ n | 凯尔希 |
| `PeerTagInRoom(tag)` | 同房存在带 `tag` 的其他干员 | 火龙S黑角、麒麟R夜刀 |
| `TiandaoEffVarAllowed` | 市井之道 + 天道酬勤互斥规则 | 孑+雪雉 |
| `PartnerInRoom(name)` | 同房存在某干员名 | 通用 |
| `TagPresentInRoom(tag)` | 同房存在某 tag | 通用 |
| `OperatorInBase(name)` | 某干员在全基建中 | 通用 |
| `ActiveRecipe(kind)` | 当前配方类型 | 制造站 |
| `GoldOrderInvestEligible` | 赤金、交付 >3、无 breach tag | 龙舌兰 |

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

## 八、分层求解概要

v2 不是「只有一个 interpreter」。贸易站求解是**三层协作**：

```
L1 主路径：interpreter（Phase 排序 → Selector/Condition/Action）
L2 域短路：gold_flow（赤金链）、order_mechanic（订单分布→等效效率）
L3 组合短路：shortcut + trade_shortcuts.json（表化最优解）
```

详细设计见 [`PROJECT_MAP.md`](PROJECT_MAP.md)「求解流水线」及 `trade/solver.rs`。

**委托标记**：`skill_table.json` 中 `atoms: []` 表示「已注册，执行权委托给域引擎」，**不是未建模**。

---

### 8.13 全局资源注册表

代码真相源：`global_resource/registry.rs` 的 `REGISTRY` / `CONVERSIONS`。

| `GlobalResourceKey` | 中文 | 典型 producer | 典型 consumer | 阶段 |
|---------------------|------|---------------|---------------|------|
| `Matatabi` | 木天蓼 | 中枢·火龙S黑角 / 麒麟R夜刀 | 泰拉大陆调查团 | P0 |
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

**已知转化边**：梦境/小节/记忆碎片 →感知 1:1；感知→无声共鸣 1:1；感知→思维链环 1:1；人间烟火→巫术结晶 5:1。

---

## 九、跨设施编排层

### 9.1 问题

跨设施效果（宿舍产感知、办公室产记忆碎片、贸易站黑键自产感知、乌有自产烟火）在 `layout/resolve.rs` 中以按名硬编码的方式注入全局池。每次新增跨房干员需要在 `resolve.rs` 打补丁。

### 9.2 方案

引入 `AtomScope` 枚举区分同房（`Room`）和跨房（`Global`）atom。

- `scope: room`（默认）— 现有行为，per-room 求解执行
- `scope: global` — 由新建的 `cross_facility/` 编排层统一执行，per-room 求解跳过（避免重复计数）

```
resolve_base 执行顺序（新增阶段 5）:

  1. WorkforceIndex 建索引 + layout stats
  2. 中枢求解（全局注入 + 资源生产）
  3. 发电站求解（状态池写入）
  4. 办公室求解
  5. cross_facility 编排 ← 新增
     ├─ collect_global_atoms（全基建 scope=Global atom）
     └─ orchestrate_global_atoms（按 Phase 排序执行 → GlobalResourcePool）
  6. run_conversions（全局资源转化）
  7. per-room 求解（trade/manufacture/power）
```

### 9.3 核心类型

| 类型 | 位置 | 职责 |
|------|------|------|
| `AtomScope::Global` | `types.rs` | 标记跨房 atom |
| `GlobalAtomEntry` | `cross_facility/collector.rs` | 收集的跨房 atom + 元信息 |
| `GlobalResourceSnapshot` | `cross_facility/mod.rs` | 编排输出（全局池 + 注入 + layout） |
| `collect_global_atoms` | `cross_facility/collector.rs` | 全基建扫描收集 |
| `orchestrate_global_atoms` | `cross_facility/interpreter.rs` | 执行编排 |

### 9.4 执行范围

| Phase | 处理 | 说明 |
|-------|------|------|
| `StateWrite` | ✅ 处理 | `StateProduce`、`StateConvert` |
| `GlobalInject` | ⛔ 跳过 | 仍由 `control/interpreter.rs` 管理 |
| `Constant`/`EffVar` 等 | ⛔ 跳过 | 对跨设施场景无意义 |

### 9.5 目前已迁移的 cross-facility Selector

| Selector | 说明 |
|----------|------|
| `DormOccupantCount` | 宿舍人数（黑键/乌有/迷迭香产量因子） |
| `FacilityLevel` | 设施等级（爱丽丝/车尔尼/森西产量因子） |
| `TradeStationCount` | 贸易站数量 |
| `PowerStationCount` | 发电站数量 |
| `DormLevelSum` | 宿舍有效等级之和（用于“每间宿舍每级”） |
| `MeetingMaxLevel` | 会客室最高等级 |
| `EliteFacilityCount` | 精英干员设施数 |
| `SuiFacilityCount` | 岁设施数 |

### 9.6 迁移路径

| 阶段 | 内容 | 状态 |
|------|------|------|
| **P1** | 基础设施部署（`AtomScope` + `cross_facility/` + `resolve.rs` 集成） | ✅ 已完成 |
| **P2** | 迁移 resolve.rs 硬编码到 scope=global atom（乌有/森西/爱丽丝/车尔尼/絮雨） | ⬜ 待做 |
| **P3** | 删除 resolve.rs 旧硬编码函数 + room_layout 扣回 | ⬜ 待做 |

P2 迁移示例：在 `skill_table.json` 中乌有的 `trade_ord_spd_bd_n2[000]` 的 `state_write` atom 加 `"scope": "global"` 后，`cross_facility` 自动执行该 atom 写入全局池，然后从 `resolve.rs` 删除 `apply_wuyou_human_fireworks_baseline` 函数即可。
