# 编排引擎（Orchestration Engine）设计文档

> 状态：初稿
> 作者：Agent Architect
> 日期：2026-06-15

---

## 一、问题陈述

现有 L1-L2-L3 三层架构是**单房视角**，而真实需求涉及**跨房协作权衡**：

```
L1 interpreter:    这个技能怎么算？                   单房
L2 gold_flow/om:   这个单房的等效效率是多少？         单房
L3 shortcut:       这个组合的已知最优解？             单房
----------------------------------------------------------
缺失的层：            全编制方案之间的权衡比较          跨房
```

一旦出现需要跨房权衡的干员（黑键感知+贸易、森蚺中枢+制造、清流跨设施20%/贸），Agent 只能打硬编码补丁：

- [`try_colocate_blackkey_with_meta`](crates/infra-core/src/layout/assign.rs:865)（85 行手写两房评分）
- `try_assign_gongsun_gold_manu_team`（固定组合硬编码）
- `assign_trade_meta` 中的硬编码 `[("witch", ...), ("closure", ...)]`

**根源**：五个体系（迷迭香/自动化/红松林/莱茵/叙拉古）共享同一个抽象结构——固定核心 + 可降级 + 跨设施绑定 + 与其他体系互斥——但代码里没有表达这个结构的通用层。

---

## 二、核心抽象：System

### 2.1 公孙长乐五份反馈的模式归纳

公孙长乐的五份反馈揭示了统一的模式谱系：

```
System = {
    硬核心（缺一不可启动）
    producer 前提条件（满足才能达到某档效率）
    固定 slot（核心干员预占的房间）
    效率散件 slot（贪心补齐的房间 和 角色）
    跨设施身份冲突描述
    与其它体系的互斥关系
    降级档（producer 减少时的效率/priority 递减链）
    班次偏好（peak-only / recovery / 全体）
}
```

### 2.2 统一概念模型

```
┌──────────────────────────────────────────────────────────────┐
│ System                                                       │
│ ├─ id / label / priority / shift_modes                      │
│ ├─ exclusive_group                                          │
│ ├─ prerequisites: [ProducerCondition]     // 可选的producer前提  │
│ ├─ required_operators: [OperatorCheck]    // 硬核心干员存在性检查 │
│ ├─ slots: [SlotDef]                      // 房间固定绑定+散件贪心 │
│ └─ tiers: [SystemTier]                   // 降级档             │
│      ├─ priority: i32                    // 降级后优先级       │
│      ├─ prerequisites_met: &[str]         // 引用上层条件       │
│      └─ optional_slots: [SlotDef]         // 裁剪掉的slot       │
└──────────────────────────────────────────────────────────────┘
```

---

## 三、数据驱动 Schema（`base_systems.json` 扩展）

### 3.1 新增字段（加粗为扩展）

| 字段 | 类型 | 说明 | 当前状态 |
|------|------|------|---------|
| `id` | string | 体系标识 | ✅ 已有 |
| `label` | string | 中文描述 | ✅ 已有 |
| `priority` | i32 | 默认优先级 | ✅ 已有 |
| `shift_modes` | string[] | 班次启用 | ✅ 已有 |
| `exclusive_group` | string? | 互斥组 | ✅ 已有 |
| `segment_id` | string? | 关联 trade_segments | ✅ 已有 |
| **`prerequisites`** | `PrerequisiteDef[]` | **前置条件** | ❌ 新增 |
| **`required_operators`** | `OperatorCheck[]` | **硬核心干员检查** | ❌ 新增 |
| **`tiers`** | `SystemTierDef[]` | **降级档** | ❌ 新增（替代当前单一 system）|
| `slots` | `SlotDef[]` | 房间绑定 | ✅ 已有 |

### 3.2 新增类型定义

```jsonc
// PrerequisiteDef: producer 前提条件
{
  "id": "effective_power_stations_gte",  // 条件标识
  "value": 4,                            // 条件参数
  "source": "layout"                     // 数据来源：layout / inject / operbox
}

// OperatorCheck: 硬核心干员存在性检查
{
  "name": "黑键",
  "elite": 2,
  "required": true,                      // true=缺则整链跳过
  "check": "operbox"                     // 从 operbox 检查拥有
}

// SystemTierDef: 降级档
{
  "id": "rosemary_perception_tier3",
  "priority": 15,                         // 降级后 priority
  "label": "感知链·档3：仅三核心",         // 降级后 label
  "prerequisites_met": ["no_dorm_producers"],  // 哪些条件未满足触发此降级
  "slots": [ /* 降级后的 slot 子集 */ ]
}
```

### 3.3 SlotDef 增强

| 字段 | 当前 | 扩展 |
|------|------|------|
| `optional` | ✅ 已有 | 不变 |
| `operators` | ✅ 已有 | 不变 |
| `facility` | ✅ 已有 | 不变 |
| `room_id` | 🟡 可选指定 | 新增 `room_kind: string?`（按设施类型自动分配） |
| `fill_mode` | ❌ | 新增 `greedy` / `fixed` / `pick_one` 区分核心与散件 |

**`fill_mode` 语义**：

| 值 | 含义 | 示例 |
|----|------|------|
| `fixed` | 固定干员，缺则跳过整链 | 叙拉古但书三人 |
| `core` | 固定单人锚点，队友贪心补齐 | 黑键定贸站、迷迭香定制造站 |
| `greedy` | 全部贪心，不绑干员 | 普通贸易/制造站 |
| `pick_one` | 从列表中选第一个可用 | 喀兰工具人[琳琅诗怀雅/崖心/锏/讯使] |

### 3.4 完整示例：迷迭香感知链

```json
{
  "id": "rosemary_perception",
  "label": "迷迭香感知链：黑键+迷迭香+感知producer堆叠",
  "base_priority": 21,
  "shift_modes": ["peak"],
  "required_operators": [
    { "name": "迷迭香", "elite": 2, "required": true },
    { "name": "黑键", "elite": 2, "required": true }
  ],
  "tiers": [
    {
      "id": "rosemary_tier1",
      "label": "感知链·满配：含夕+宿舍+絮雨",
      "priority": 21,
      "prerequisites_met": [],
      "slots": [
        { "facility": "trade_post", "fill_mode": "core",
          "operators": [{ "name": "黑键", "elite": 2 }] },
        { "facility": "factory", "fill_mode": "core",
          "operators": [{ "name": "迷迭香", "elite": 2 }] },
        { "facility": "control", "fill_mode": "fixed",
          "operators": [{ "name": "夕", "elite": 0 }], "optional": true },
        { "facility": "office", "fill_mode": "fixed",
          "operators": [{ "name": "絮雨", "elite": 2 }], "optional": true },
        { "facility": "dormitory", "fill_mode": "fixed",
          "operators": [{ "name": "爱丽丝", "elite": 2 }], "optional": true },
        { "facility": "dormitory", "fill_mode": "fixed",
          "operators": [{ "name": "车尔尼", "elite": 2 }], "optional": true }
      ]
    },
    {
      "id": "rosemary_tier2",
      "label": "感知链·档2：无宿舍producer",
      "priority": 18,
      "prerequisites_met": ["no_dorm_producers"],
      "slots": [
        { "facility": "trade_post", "fill_mode": "core",
          "operators": [{ "name": "黑键", "elite": 2 }] },
        { "facility": "factory", "fill_mode": "core",
          "operators": [{ "name": "迷迭香", "elite": 2 }] },
        { "facility": "control", "fill_mode": "fixed",
          "operators": [{ "name": "夕", "elite": 0 }], "optional": true },
        { "facility": "office", "fill_mode": "fixed",
          "operators": [{ "name": "絮雨", "elite": 2 }], "optional": true }
      ]
    },
    {
      "id": "rosemary_tier3",
      "label": "感知链·档3：仅三核心+絮雨",
      "priority": 15,
      "prerequisites_met": ["no_control_producers"],
      "slots": [
        { "facility": "trade_post", "fill_mode": "core",
          "operators": [{ "name": "黑键", "elite": 2 }] },
        { "facility": "factory", "fill_mode": "core",
          "operators": [{ "name": "迷迭香", "elite": 2 }] },
        { "facility": "office", "fill_mode": "fixed",
          "operators": [{ "name": "絮雨", "elite": 2 }], "optional": true }
      ]
    }
  ]
}
```

### 3.5 自动化组示例（含 producer 前提）

```json
{
  "id": "automation_group",
  "label": "自动化组：温蒂E2+清流+森蚺+承曦格雷伊 赤金专精",
  "base_priority": 20,
  "shift_modes": ["peak"],
  "required_operators": [
    { "name": "温蒂", "elite": 2, "required": true },
    { "name": "清流", "elite": 2, "required": true }
  ],
  "prerequisites": [
    { "id": "effective_power_stations", "value": 4, "source": "layout" }
  ],
  "tiers": [
    {
      "id": "auto_tier_full",
      "label": "自动化满配：承曦格雷伊+森蚺，4有效电站，140%",
      "priority": 20,
      "prerequisites_met": [],
      "slots": [
        { "facility": "power_plant", "fill_mode": "fixed",
          "operators": [{ "name": "承曦格雷伊", "elite": 2 }] },
        { "facility": "factory", "fill_mode": "fixed",
          "operators": [
            { "name": "清流", "elite": 2 },
            { "name": "温蒂", "elite": 2 }
          ] }
      ]
    },
    {
      "id": "auto_tier_t1",
      "label": "自动化档1：无承曦格雷伊，3物理电站，~90%",
      "priority": 14,
      "prerequisites_met": ["no_virtual_power"],
      "slots": [
        { "facility": "factory", "fill_mode": "fixed",
          "operators": [
            { "name": "清流", "elite": 2 },
            { "name": "温蒂", "elite": 2 }
          ] }
      ]
    }
  ]
}
```

---

## 四、编排引擎流水线

### 4.1 流水线总图

```
                     ┌───────────────────┐
                     │   blueprint        │
                     │ + operbox          │
                     │ + instances/table  │
                     └────────┬──────────┘
                              ↓
   ╔══════════════════════════╗
   ║  阶段一：System候选枚举    ║
   ║  ┌─────────────────────┐ ║
   ║  │逐个 System 按 priority│ ║
   ║  │  ├ required_operators│ ║
   ║  │  ├ prerequisites    │ ║
   ║  │  ├ tiers(降级档)     │ ║
   ║  │  └→ 生成 ResolvedSys │ ║
   ║  │互斥组只取最高priority │ ║
   ║  └─────────────────────┘ ║
   ╚══════════════════════════╝
                ↓
   ╔══════════════════════════╗
   ║  阶段二：候选方案穷举      ║
   ║                          ║
   ║  每个 "多身份干员" 枚举   ║
   ║  它的 K 种安置方案:       ║
   ║  ┌──────────────────┐   ║
   ║  │黑键A: 黑键+但书同站│   ║
   ║  │黑键B: 黑键+可露希站│   ║
   ║  │黑键C: 黑键独站     │   ║
   ║  │方案数 = ∏K ≤ 可控  │   ║
   ║  └──────────────────┘   ║
   ║                          ║
   ║  每个候选方案：           ║
   ║  按 claim_systems +      ║
   ║  贪心落位生产设施         ║
   ║  → BaseAssignment        ║
   ╚══════════════════════════╝
                ↓
   ╔══════════════════════════╗
   ║  阶段三：评分裁决          ║
   ║                          ║
   ║  resolve_base → solve    ║
   ║  DailyTotals             ║
   ║     ├ trade              ║
   ║     ├ manu               ║
   ║     └ power              ║
   ║                          ║
   ║  ScoringPolicy            ║
   ║  (公孙平衡曲线，TBD)      ║
   ║      ↓                    ║
   ║  单一标量 → 排序          ║
   ║  argmax → 最优方案        ║
   ╚══════════════════════════╝
                ↓
         最优 BaseAssignment
```

### 4.2 阶段一：System 候选枚举

输入：`base_systems.json` 全部 system + 当前 `operbox` + `blueprint` + `GlobalInjectManifest`

逻辑：

1. 按 `base_priority` 排序所有 system
2. 对每个 system：
   - 检查 `required_operators`（硬核心）
   - 检查 `prerequisites`（producer 前提）
   - 根据前提满足情况选择 `tiers` 中匹配的降级档
   - 检查 `slots` 可行性
   - 若 `exclusive_group` 已被更高 priority system 认领 → 跳过
3. 输出：`Vec<ResolvedSystem>`（已确认可认领的体系及其降级档）

```rust
pub struct ResolvedSystem {
    pub id: String,
    pub tier_id: String,
    pub priority: i32,
    pub slots: Vec<ResolvedSlot>,  // 确定落位的 slot
    pub pending_anchors: Vec<PendingAnchor>,  // 多身份干员，待方案枚举
}
```

### 4.3 多身份干员（跨房权衡的来源）

**核心概念**：一个干员如果在多个 System 中同时作为核心（或被引用），需要在候选枚举时**浮动**它的安置方案。

当前已知的多身份干员：

| 干员 | 身份 1 | 身份 2 | 安置选择 |
|------|--------|--------|---------|
| 黑键 | 迷迭香感知 core | 贸易站第三人 | 放但书站 / 放可露希尔站 / 独站 |
| 森蚺 | 自动化组第三人 | 中枢 VirtualPower | 制造站（243）/ 中枢（252/342）|
| 砾 | 红松林赤金线 | 通用制造散件 | 红松林站 / 其他站 |
| 清流 | 自动化组 core | 跨设施 20%/贸 | 自动化组固定 |

**候选枚举规则**：对每个多身份干员，它的安置方案数量 = `alternative_slot_count`（引擎不暴力组合所有排列，只枚举 System 间冲突的自然交叉点）。

### 4.4 阶段二：候选方案生成

1. 固定所有 `slots[fill_mode=fixed]`
2. 对 `fill_mode=core` 的 slot（黑键、迷迭香），标记为 `PendingAnchor`
3. 对多身份干员，枚举 K 种方案（通常是 2-3 种）
4. 每方案：
   - claim 所有 system 的 fixed slot
   - 锚点 slot 按方案固定
   - 剩余生产设施贪心落位（复用当前 `assign_shift` 逻辑）
   - 输出完整 `BaseAssignment`

**方案数估算**：
```
方案数 ≈ 多身份干员数 × 每个的 alternatives
     ≤ 3（当前）× 2-3 = 6-9 个方案
```

这是可控的，不违反 "不做全局笛卡尔积" 原则。

### 4.5 阶段三：评分裁决

```rust
pub trait ScoringPolicy: Send + Sync {
    /// 对完整 BaseAssignment 返回单一标量（越高越优）。
    fn score(&self, resolved: &ResolvedBase, daily: &DailyTotals) -> f64;
}
```

- **默认实现**：当前语义——trade/manu/power 分类独立不混合，引擎保留各方案的三类产出明细供外部工具分析
- **公孙平衡曲线实现**：TBD，作为 `ScoringPolicy` 插件注入

```
最终输出格式：
  ranked_plans: Vec<{
    plan_index: usize,
    assignment: BaseAssignment,
    daily: DailyTotals,
    score: f64,
    claimed_systems: Vec<String>,  // 认领的 system id 列表
  }>
  winner: plan_index  // argmax
```

---

## 五、Rust 类型草图

### 5.1 数据层（`base_systems.json` 映射）

```rust
/// 扩展后的 base_systems.json 解析类型
#[derive(Debug, Clone, Deserialize)]
struct BaseSystemsFile {
    systems: Vec<SystemDef>,
}

#[derive(Debug, Clone, Deserialize)]
struct SystemDef {
    id: String,
    label: String,
    #[serde(default)]
    base_priority: i32,
    #[serde(default)]
    shift_modes: Vec<String>,
    #[serde(default)]
    segment_id: Option<String>,
    #[serde(default)]
    exclusive_group: Option<String>,
    #[serde(default)]
    required_operators: Vec<OperatorCheck>,
    #[serde(default)]
    prerequisites: Vec<PrerequisiteDef>,
    tiers: Vec<SystemTierDef>,
}

#[derive(Debug, Clone, Deserialize)]
struct SystemTierDef {
    id: String,
    label: String,
    priority: i32,
    #[serde(default)]
    prerequisites_met: Vec<String>,  // 哪些条件未满足触发此档
    slots: Vec<SlotDef>,
}

#[derive(Debug, Clone, Deserialize)]
struct PrerequisiteDef {
    id: String,
    #[serde(default)]
    value: f64,
    source: String,  // "layout" | "inject" | "operbox"
}

#[derive(Debug, Clone, Deserialize)]
struct OperatorCheck {
    name: String,
    elite: u8,
    #[serde(default = "default_required_true")]
    required: bool,
}

fn default_required_true() -> bool { true }

#[derive(Debug, Clone, Deserialize)]
struct SlotDef {
    facility: String,
    #[serde(default)]
    room_id: Option<String>,
    #[serde(default)]
    fill_mode: FillMode,
    #[serde(default)]
    optional: bool,
    operators: Vec<SystemOperatorSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FillMode {
    Fixed,
    Core,      // 固定单人锚点，队友贪心补齐
    Greedy,    // 全部贪心
    PickOne,   // 从 pick_one 列表选
}
```

### 5.2 运行时层（`crates/infra-core/src/orchestrate/`）

```rust
// crates/infra-core/src/orchestrate/mod.rs
mod plan;
mod solver;

pub use plan::{PlanGenerator, ResolvedSystem, CandidatePlan};
pub use solver::{OrchestrationResult, ScoringPolicy, DefaultScoringPolicy};

/// 编排引擎入口
pub fn orchestrate(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    scorer: &dyn ScoringPolicy,
) -> Result<OrchestrationResult>;
```

```rust
// crates/infra-core/src/orchestrate/plan.rs
/// 阶段一+二输出：一个候选方案
pub struct CandidatePlan {
    pub index: usize,
    pub claimed_systems: Vec<ResolvedSystem>,
    pub assignment: BaseAssignment,
}

/// 候选方案生成器
pub struct PlanGenerator {
    systems: Vec<SystemDef>,
}

impl PlanGenerator {
    /// 枚举所有候选方案
    pub fn enumerate(
        &self,
        blueprint: &BaseBlueprint,
        operbox: &OperBox,
        instances: &OperatorInstances,
        table: &SkillTable,
        options: &AssignBaseOptions,
    ) -> Result<Vec<CandidatePlan>>;
}
```

```rust
// crates/infra-core/src/orchestrate/solver.rs
pub struct OrchestrationResult {
    pub plans: Vec<ScoredPlan>,
    pub winner: usize,
    pub elapsed: Duration,
}

pub struct ScoredPlan {
    pub plan_index: usize,
    pub assignment: BaseAssignment,
    pub daily: DailyTotals,
    pub score: f64,
    pub claimed_systems: Vec<String>,
}

pub trait ScoringPolicy: Send + Sync {
    fn score(&self, blueprinte: &BaseBlueprint, daily: &DailyTotals) -> f64;
}

pub struct DefaultScoringPolicy;

impl ScoringPolicy for DefaultScoringPolicy {
    fn score(&self, _blueprint: &BaseBlueprint, daily: &DailyTotals) -> f64 {
        // 当前保留分类输出，不混合量纲
        // 当 user 未提供平衡曲线时，默认为 "不裁决，输出各方案明细由人工判断"
        // 但 engine 仍按 trade（最高优先级）做默认排序
        0.0
    }
}
```

---

## 六、分配路径：消化现有补丁

### 6.1 一步到位吃掉 `try_colocate_blackkey_with_meta`

当前补丁（85 行）手工做了三件事：

| 补丁做的事 | 编排引擎接管方式 |
|------------|----------------|
| 检查但书/可露希尔站是否可用 | System 候选枚举自动处理 |
| 枚举黑键的第三人候选 | 多身份干员候选枚举 |
| 两房 solve + 比较 score | 全局评分裁决 |

**迁移后**：`try_colocate_blackkey_with_meta` 整函数删除。

### 6.2 吃掉 `try_assign_gongsun_gold_manu_team`

当前补丁是 `["清流", "温蒂", "森蚺"]` 固定组合。

迁移后：`base_systems.json` 的 `automation_group` 包含此组合。引擎在 claim 时会自动占用该制造站。`try_assign_gongsun_gold_manu_team` 和 `GONGSUN_GOLD_MANU_TEAM` 常量删除。

### 6.3 简化 `assign_trade_meta`

当前 `assign_trade_meta` 在 Rust 代码里写了 [`("witch", hit_witch_shortcut), ("closure", hit_closure_shortcut)`](crates/infra-core/src/layout/assign.rs:580) 的硬编码 fallback 链。

迁移后：System 候选枚举 + `fill_mode=core` 锚点自动处理「巫恋站、可露希尔站」的认领优先级。`assign_trade_meta` 简化为通用贪心（或删除，由 `assign_trade_remainder` 统一处理）。

### 6.4 简化 `claim_base_systems`

当前 [`claim_base_systems`](crates/infra-core/src/layout/system.rs:226) 是单 pass 的 "全有全无" 贪心认领。需要改为：

1. **第一阶段**：system 枚举 + 降级判断 + 互斥裁决 → 生成 `Vec<ResolvedSystem>`
2. **第二阶段**：锚点落位 + 候选方案展开
3. `claim_base_systems` 自身 = 对第一阶段输出的 fixed slot 做实际 `set_room` + `used.insert`

---

## 七、分阶段实现清单

### 阶段一：数据层扩展（不动 Rust）

| 任务 | 改什么 | 影响 |
|------|--------|------|
| 1. 扩展 `base_systems.json` schema | `SystemDef` 增加 `tiers`、`prerequisites`、`required_operators` | 数据结构 |
| 2. 迷迭香链按新 schema 重写 | 3 个 tier，core slot，optional sense producer | 数据文件 |
| 3. 自动化组按新 schema 重写 | 2 个 tier（满配/降级）+ `prerequisites` | 数据文件 |
| 4. 红松林按新 schema 重写 | 2 个 tier（满配/不完整）+ `required_operators` | 数据文件 |
| 5. 叙拉古但书按新 schema 重写 | 1 tier，`segment_id` 引用 | 数据文件 |
| 6. 巫恋/喀兰/推王/怪猎 同 | 简洁化 | 数据文件 |
| 7. 删除 `GONGSUN_GOLD_MANU_TEAM` 等硬编码常量 | 全部迁入 JSON | `assign.rs` |

### 阶段二：运行时引擎骨架（新建 `orchestrate/`）

| 任务 | 改什么 | 影响 |
|------|--------|------|
| 1. 创建 `crates/infra-core/src/orchestrate/` 模块 | `mod.rs` | 新文件 |
| 2. 实现 `SystemDef` 序列化扩展（tiers/prerequisites fill_mode） | system.rs | 重构 |
| 3. 实现阶段一：`enumerate_systems`（降级档选择+互斥裁决） | `plan.rs` | 新功能 |
| 4. 实现阶段二：`PlanGenerator::enumerate` + 多身份干员候选枚举 | `plan.rs` | 新功能 |
| 5. 实现阶段三：评分 + 方案排序 | `solver.rs` | 新功能 |
| 6. `ScoringPolicy` trait + `DefaultScoringPolicy` | `solver.rs` | 新功能 |

### 阶段三：流水线替换 `assign_shift`（主要重构）

| 任务 | 改什么 | 影响 |
|------|--------|------|
| 1. 用编排引擎替换 `assign_shift` 开头 | `assign.rs` | 重构 |
| 2. 删除 `try_colocate_blackkey_with_meta` | `assign.rs` | 删除 85 行 |
| 3. 删除 `try_assign_gongsun_gold_manu_team` | `assign.rs` | 删除 |
| 4. 删除 `GONGSUN_GOLD_MANU_TEAM` / `ROSEMARY_MANU_TEAM` | `assign.rs` | 删除常量 |
| 5. 简化 `assign_trade_meta` | `assign.rs` | 提取到 System 候选 |
| 6. 保留 `assign_control` / `assign_dorm_producers` / `assign_power` | 不变 | 可独立于编排 |
| 7. 更新 `claim_base_systems` → 调用编排引擎阶段一 | `system.rs` | 重构 |

### 阶段四：测试与回归

| 任务 | 改什么 | 影响 |
|------|--------|------|
| 1. 编排引擎单元测试 | `orchestrate/tests.rs` | 新增 |
| 2. system 降级档选择测试 | system 测试 | 补充 |
| 3. `verify --all` 回归通不过回退 | 回归 CSV | 按需调整 |
| 4. 243 全精2 编排 vs 当前 hardcoded assign 对照 | `layout test` | 对对碰 |

### 阶段五：后续增强

| 任务 | 说明 |
|------|------|
| 1. `ScoringPolicy` 插件化（公孙平衡曲线） | 评分函数可外部配置 |
| 2. blueprint 自动推导 producer 前提 | 如有效电站数=物理+虚拟，自动计算 |
| 3. system 降级档的自动化测试 | 验证 `base_systems.json` 所有 tier 可正确选择 |
| 4. assign.rs 内部地图文档 | `docs/INTERNAL/ASSIGN.md` |

---

## 八、与现有架构的关系

### 8.1 新增的层

```
L1 interpreter                   单房技能求值           不变
L2 gold_flow / order_mechanic    单房域引擎             不变
L3 shortcut                      单房组合短路           不变
──────────────────────────────────────────────────────────
L4 orchestrate                   跨房方案枚举+评分裁决    新增
```

### 8.2 不变的

- L1-L2-L3 完全不变
- `search/*.rs` 完全不变（单房搜索）
- `pool/*.rs` 完全不变
- `control/interpreter.rs` 完全不变
- `resolve.rs` 完全不变
- `schedule/` 对外 API 不变（但内部调用编排引擎）

### 8.3 变更的文件

| 文件 | 变化 |
|------|------|
| `base_systems.json` | schema 扩展 + 全部 system 按新 schema 重写 |
| `layout/system.rs` | `SystemDef` 解析扩展 + `enumerate_systems` |
| `layout/assign.rs` | 删除 `try_colocate_*` 等补丁，流水线简化为编排引擎调用 |
| `layout/mod.rs` | 导出 `orchestrate` 模块 |
| `search/role_pick.rs` | hit_filter bridge 改为 trait 或保持现状（可选） |
| `orchestrate/` | 新建 |
| `docs/EFFECT_ATOM_DESIGN.md` | 新增 §8.14 编排层 |
| `docs/INTERNAL/ASSIGN.md` | 新建内部地图 |

### 8.4 不碰的

- `shortcut.rs` 的干员名匹配（L3 允许）
- `pool/trade.rs` 的孑特例（设计合理，维持现状）
- `REGRESSION_CASES.csv` 的双轨制（另开任务）

### 8.5 对接质量审计

#### 完美对接（4/9）

| 接口 | 当前代码 | 对接方式 |
|------|---------|---------|
| 评分函数 | [`score_base_assignment`](crates/infra-core/src/schedule/base_rotation.rs:84) 已完整实现 resolve→逐房 solve→`ShiftScores` | 编排引擎阶段三直接调用，`ScoringPolicy` trait 包装 |
| `DailyTotals` | [`team_rotation.rs:58`](crates/infra-core/src/schedule/team_rotation.rs:58) 已有 `{ trade, manu, power }` | 不变，编排引擎输出同结构 |
| `BaseAssignment` | [`assignment.rs:38`](crates/infra-core/src/layout/assignment.rs:38) 已有 rooms/set_room/operators_in | 编排引擎输入输出都用它，不变 |
| `resolve_base` | [`resolve.rs:67`](crates/infra-core/src/layout/resolve.rs:67) 已接受 blueprint+assignment+instances+table+mood | 编排引擎对每个候选方案调它，签名一致 |

#### 需适配的（3/9）

| 接口 | 当前代码 | 适配方式 |
|------|---------|---------|
| `claim_base_systems` | [`system.rs:226`](crates/infra-core/src/layout/system.rs:226) 全有全无贪心认领 | 改为两步：① 编排引擎选 tier ② system.rs 只做实体的 set_room + used.insert。核心逻辑 `slot_resolvable`/`resolve_slot_operators` 可复用 |
| `assign_shift` | [`assign.rs:72`](crates/infra-core/src/layout/assign.rs:72) ~180 行流水线 | 编排引擎接管前半（claim_system→锚点落位），后半（assign_power/manu_remainder）复用现有函数。`assign_shift` 变为编排引擎的薄 wrapper |
| `prerequisite 推导` | [`context.rs:26`](crates/infra-core/src/layout/context.rs:26) 有 `power_station_count`，`global` 池有 `VirtualPower` | 新增 `effective_power_stations()` 辅助函数给 `PrerequisiteDef` 的 `source: "layout"` 分支 |

#### 有隐形摩擦的（2/9）

**摩擦点 A：resolve.rs 的感知 producer 注入是硬编码的**

[`resolve.rs:281`](crates/infra-core/src/layout/resolve.rs:281)：

```rust
const DORM_PERCEPTION_PRODUCERS: &[&str] = &["爱丽丝", "车尔尼"];
```

编排引擎裁剪掉某个 dorm producer slot 后，`resolve_base` 仍会自动扫描全 assignment 注入所有感知 producer。如果编排引擎选了感知链的降级档（无爱丽丝/车尔尼），resolve 不会自动感知到。

**处理方式**：不侵入 `resolve_base`，由编排引擎在 resolve 后对 `layout.global.Perception` 做减法。具体来说——编排引擎知道当前 tier 启用了哪些感知 producer，而 `resolve_base` 不加区分地注入了全部。编排引擎在拿到 resolved 后，计算差值并减去未启用 producer 的感知量。这是一个「事后矫正」模式，对 resolve 零侵入。

**摩擦点 B：base_systems.json schema 迁移的向后兼容**

当前 9 个 system 条目都是平铺的 `BaseSystemDef`，新 schema 改为 `tiers: [SystemTierDef]` 结构。迷迭香链从 1 个条目变成 3 个 tier 内嵌。

**处理方式**：全部旧字段保留，新增字段加 `#[serde(default)]`。`tiers` 非空时使用新逻辑；`tiers` 为空时回退到旧单 system 行为。这是零破坏过渡，阶段一可并行运行。

#### 审计总结

```
对接总评

完美对接  ████████████████░░░░  4/9  44%
需适配   ████████████░░░░░░░░  3/9  33%
有摩擦   ████████░░░░░░░░░░░░  2/9  22%

TOTAL:  7/9 对接良好（78%）
        2/9 有摩擦（22%，已标明处理方式）
```

### 8.6 对 team_rotation 的影响

当前 [`team_rotation.rs`](crates/infra-core/src/schedule/team_rotation.rs) 已经有一个 αβγ 三队轮换实现，它直接调用 `assign_shift` 获取高峰班，然后切半。

**编排引擎替换 `assign_shift` 后**：

- `assign_shift` 变为编排引擎的薄 wrapper → `team_rotation.rs` 无需改动调用接口，但内部拿到的 assignment 质量会因 System 候选枚举而提升
- `team_rotation.rs` 的 `split_production_facilities` 切半逻辑保持不变
- 长远看，`team_rotation.rs` 也可以消费编排引擎的多个候选方案来做「每班走不同方案」——但这属于阶段五增强，不阻塞

---

## 九、设计原则回顾

| 原则 | 本设计是否符合 |
|------|--------------|
| **不做全局联合最优（禁止笛卡尔积）** | ✅ 候选方案数 ≤ 10，不组合爆炸 |
| **贪心 + top_k 回退** | ✅ 阶段一仍是 priority 贪心，但多了降级档 |
| **机制在 core / CLI 只编排** | ✅ 编排引擎在 `infra-core::orchestrate`，CLI 调它 |
| **ScoringPolicy 可插拔** | ✅ trait 解耦 |
| **不与心情排班耦合** | ✅ 编排引擎输出 BaseAssignment，不关心心情 |
| **数据驱动优先** | ✅ System 谱系全部在 JSON 中，Rust 只做匹配和落位 |
