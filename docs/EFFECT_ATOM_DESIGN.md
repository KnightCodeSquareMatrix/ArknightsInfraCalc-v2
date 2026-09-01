# EffectAtom 设计文档

> 文档角色：canonical
> 生命周期状态：current
> 领域键：mechanics.effect-atom
> 当前真源：self
> 摘要：裁决 EffectAtom 词汇和机制解释分层

> 本文档记录 ArknightsInfraCalc 重建模的核心设计。
> **已建模干员详情**见 [`MODELLED_OPERATORS.md`](MODELLED_OPERATORS.md)。
> **体系链**见 [`SYSTEM_CHAINS.md`](SYSTEM_CHAINS.md)。
> **改机制协作流程**见 [AGENTS.md 的“核心分层边界”](../AGENTS.md#4-核心分层边界)；准备实现事项见 [`TODO/`](TODO/)。

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
    atom_id: Option<String>,       // 稳定来源身份；JSON 当前字段名为 `id`
    selector: Selector,      // 从哪取数
    action: Action,          // 做什么计算
    condition: Option<Condition>,  // 什么情况下触发
    tag: Option<String>,     // 可选标记，供后续 phase 引用/修改
    phase: Phase,            // 执行阶段
    phase_order: i32,        // 阶段内排序
}
```

同一个 BuffDef 可以有多个 EffectAtom，自由组合。

### 2.1 稳定 atom 身份

`atom_id` 只表示一个 atom 的稳定来源身份，不表达机制语义、效果族或聚合规则。

- JSON 使用现有的 `id` 字段；Rust 侧映射为 `atom_id`。
- 同一个 `SkillDef` 内，非空 atom 的 `atom_id` 必须唯一。
- 控制中枢的非空 atom 必须提供非空 `atom_id`；其他设施的旧数据在迁移完成前可以暂缺。
- atom 调整数组顺序、改变 `phase_order` 或扩展运行时编译信息时，不能改变既有 `atom_id`。
- `CompiledAtom.seq` 只负责当前运行时的稳定执行排序，不能作为持久身份。
- 运行时来源使用 `source_buff_id + atom_id`；禁止用数组下标、技能名或干员名生成替代身份。

已有多 atom 技能应为每个 atom 提供不同的稳定 ID。例如：

```json
{
  "id": "control_mp_psk[000]:pinus_battle_record",
  "action": { "kind": "global_inject_manu_tagged_eff" }
}
```

本节只解决 atom 身份稳定性，不引入中枢 inventory，也不改变 `atoms: []` 的委托语义。

### 2.2 上游建筑技能事实同步

`data/prts_building_skills.csv` 是从
[`yuanyan3060/ArknightsGameResource`](https://github.com/yuanyan3060/ArknightsGameResource)
的 `gamedata/excel/character_table.json` 与 `gamedata/excel/building_data.json` 同步的建筑技能事实镜像。
同步入口为 `scripts/sync_building_skills.py`，记录上游版本、角色、tier、facility、buff_id、技能名、
原文描述和解锁条件。

- 同步 CSV 可以发现本地 `operator_instances.json` 或 `skill_table.json` 的落后条目。
- 同步脚本默认只生成 CSV，不自动生成 `EffectAtom`。
- `--update-skill-table` 只更新已有 skill 的 metadata 并保留 atoms；`--add-missing` 新增项时使用
  `atoms: []`，表示已发现但尚未完成机制建模。
- 上游原文是机制事实输入，不替代 canonical 语义审阅，也不允许把概率/条件不明确的技能自动加入候选。

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
| `WitchcraftCrystal` | 巫术结晶 | 截云在自身制造房局部按 5 烟火→1 换算 | 截云制造% | P0 |
| `ThoughtChainRing` | 思维链环 | 迷迭香超感 | 迷迭香制造% | P0 |
| `IntelligenceReserve` | 情报储备 | 灰烬中枢 | 闪击/霜华/双月 | P1 |
| `UsautDrink` | 乌萨斯特饮 | 战车中枢 | 导火索、闪击、霜华 | P1 |
| `Passion` | 热情值 | 初华/祥子体系中枢 | 祥子制造%、睦贸易% | P1 |
| `EngineeringRobot` | 工程机器人 | 至简（全图扫描） | 至简机械辅助 | P2 |

**已知全局转化边**：梦境/小节/记忆碎片 → 感知 1:1。感知与人间烟火
均为共享读取资源：黑键、迷迭香以及乌有、截云等 consumer 各自读取全量且互不扣减。
感知→无声共鸣/思维链环、人间烟火→巫术结晶均在 consumer 自身房间局部换算，
不得进入全局 `run_conversions`。

**班次激活规则（2026-07-17 用户裁决）**：资源 provider 与 converter 必须在同一班实际
上岗，转换边才激活。该规则只关闭不完整链，不负责强制任一方进编，也不默认要求同房；
只有 winner 实际依赖该链时才派生同班 dependency。provider 与 converter 为同一逻辑干员/
同一 buff 时自然满足。爱丽丝梦境、车尔尼小节和絮雨记忆碎片链均按独立 buff id 建模；
絮雨的资源 atom 已从与闪击共享的 `office_rec_spd[000]` 拆到
`office_memory_fragment[000]`，不得用干员名特判。

---

## 九、跨设施编排层

### 9.1 问题

跨设施效果（宿舍产感知、办公室产记忆碎片、贸易站黑键自产感知、乌有自产烟火）在 `layout/resolve.rs` 中以按名硬编码的方式注入全局池。每次新增跨房干员需要在 `resolve.rs` 打补丁。

### 9.2 方案

引入 `AtomScope` 枚举区分同房（`Room`）和跨房（`Global`）atom。

- `scope: room`（默认）— 现有行为，per-room 求解执行
- `scope: global` — 由新建的 `cross_facility/` 编排层统一执行，per-room 求解跳过（避免重复计数）

**用户裁决（2026-08-25）**：Global atom 只由全局层执行一次；per-room 不重复执行，
也不采用“执行后从 room snapshot 扣回”的过渡模型。Global `StateConvert` 是否允许以及
其资源消耗语义仍需独立裁决；当前数据未发现 Global `StateConvert` 实例。

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
| **P3** | 删除 resolve.rs 旧硬编码函数 + per-room Global atom 执行/扣回路径 | ⬜ 待做 |

P2 迁移示例：在 `skill_table.json` 中乌有的 `trade_ord_spd_bd_n2[000]` 的 `state_write` atom 加 `"scope": "global"` 后，`cross_facility` 自动执行该 atom 写入全局池，然后从 `resolve.rs` 删除 `apply_wuyou_human_fireworks_baseline` 函数即可。

---

## 十、办公室与会客室静态求值

办公室和会客室已开放一个有界的 L2 静态求值入口，消费显式 `BaseAssignment`：

- 覆盖 `MECHANICS_REGISTRY.csv` 办公室序号 172–214、会客室序号 324–390；绑定和解锁阶段落在 `support_skill_registry.json`。
- 只结算确定性的技能速度、招募位/宿舍等级/全局资源读数、同房人数或指定搭档、时间爬升平均值和技能自身心情修正。
- 未知线索概率、下一条线索事件和线索交流状态不算分；结果通过 `ignored` 明确列出。
- 当前框架不能安全表达的跨中枢、跨宿舍、同房派系条件和心情耗尽资源生命周期不算分；结果通过 `unsupported` 明确列出。
- 中枢对办公室联络速度、会客室线索速度的确定性全局效果由 `GlobalInjectOfficeHireSpeed`、`GlobalInjectMeetingSpeed` 写入 `LayoutContext`，再由办公室/会客室求值消费；不得在 support facility 或排班层按干员名重算。当前该通道覆盖八幡海铃与祐天寺若麦的 Mujica 相关效果。
- 三角初华 E2 的中枢群体心情恢复由现有 `mood_model.json` 的 control smiley provider 读取；它不重复写入第二个心情公式。若后续需要将恢复值写入 control trace 或连续 ETA，另立 mood lifecycle 单元。
- 办公室按一人、会客室按两人容量校验；无设施技能的合法进驻者仍可参与房间人数和搭档条件。
- `resolve_base` 当前按 24 小时计算时间爬升平均值；直接 `evaluate_office` / `evaluate_meeting` 可传入其他正数时长。

会客室的最终效率在技能效率之外，按每名实际进驻干员加算基础效率：非 0 心情 `+5%`（0 心情失效）；稀有度 1–3 星 `+0%`、4 星 `+2%`、5 星 `+4%`、6 星 `+5%`；精英化 0 `+0%`、精英化 1 `+8%`、精英化 2 `+16%`。稀有度和精英化加成不受 0 心情影响，四类值与干员技能效率直接相加。该规则只属于会客室，不改变办公室效率。
- 输出是技能速度加成，不包含尚未 canonical 化的设施基础速度和线索概率收益。

会客室自动选人（2026-08-06 裁决）：单班流水线在全部生产域分配后，用剩余空闲干员把会客室填满两人；候选按 `evaluate_meeting` 的线索搜集速度加成排序（搭档条件在两人同房时自然生效），无可用技能干员时兜底任意空闲干员。ABC 轮换中会客室仍按其既有队伍规则处理；办公室则按定时换班规则逐班独立选择。仅当可占用干员不足时才保持空位（导出 `autofill: true`）。该填充只消费生产域剩余的空闲干员，不进入生产域评分，也不声称求得会客室最优编制。
