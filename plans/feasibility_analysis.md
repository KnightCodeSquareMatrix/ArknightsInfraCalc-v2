# 现有模型兼容性分析：αβγ 三队轮换

> 分析现有 `assign_shift` + `schedule_base_rotation_a_b_a` 能否适配新方案

---

## 1. 核心差异对比

| 维度 | 当前 ABA | 新 αβγ |
|------|---------|---------|
| 班次数 | 3（A→B→A'） | **3** |
| 班次时长 | 统一 8h/12h（单一值） | **12h + 6h + 6h**（不同时长） |
| 团队划分 | Peak vs Recovery（按模式） | **αβγ 三队（按站绑定）** |
| 编制生成 | 2次 `assign_shift` + 1次复用 | **3次独立编制** |
| 中枢/宿舍 | Peak 钉死给 all | **体系绑定跟队，其余轮换** |
| 每日满分 | 只用 Peak 组的分数 | **12h×αβ + 6h×βγ + 6h×γα 加权和** |

---

## 2. 可复用的现有组件

### ✅ 完整复用（无需修改）

| 组件 | 原因 |
|------|------|
| `pool/trade.rs` `build_trade_pool` `filter_trade_pool` | 池构建逻辑不变 |
| `pool/manufacture.rs` `build_manufacture_pool` | 不变 |
| `pool/power.rs` `build_power_pool` | 不变 |
| `pool/control.rs` `build_control_pool` | 不变 |
| `search/trade.rs` `search_trade_triples` | 三人组搜索不变 |
| `search/manufacture.rs` `search_manufacture_triples` | 不变 |
| `search/control.rs` `search_control_combos` | 不变 |
| `trade/solver.rs` `solve_trade_with_shift` | 单站求解不变 |
| `manufacture/solver.rs` `solve_manufacture` | 不变 |
| `layout/assignment.rs` `BaseAssignment` | 房间→干员映射格式不变 |
| `layout/blueprint.rs` `BaseBlueprint` | 蓝图定义不变 |
| `layout/resolve.rs` `resolve_base` | 统一 resolve 逻辑不变 |

### ✅ 少量修改可复用

| 组件 | 当前行为 | 改为 |
|------|---------|------|
| `layout/assign.rs` `assign_shift` | Peak/Recovery 两种模式 | 新增 `TeamAlpha/Beta/Gamma` 模式 |
| `layout/assign.rs` `assign_control` | 全量搜索中枢5人 | 支持**给定必包含列表**（体系绑定干员） |
| `layout/assign.rs` `assign_dorm_producers` | 全部贪心 | 支持指定宿舍跟队 |
| `layout/assign.rs` `assign_trade_meta` | 但书→龙巫→可露贪心 | 改为**只填指定站**（绑定站索引） |
| `layout/assign.rs` `assign_trade_remainder` | 全量贪心 | 改为只填指定站 |
| `layout/shift.rs` `AssignShiftMode` | `Peak` / `Recovery` | 新增 `TeamAlpha/Beta/Gamma` |

---

## 3. 需要新增的组件

### 3.1 `schedule/team_rotation.rs`（新房）

这是最核心的新文件，包含：

```rust
/// 方案入口
pub fn schedule_team_rotation(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
) -> Result<TeamRotationReport>
```

内部逻辑流程：

```
Step 1: 把 operbox 按设施分成 3 份 pool
  └─ 但每个队从同一个全量 pool 中搜索，通过 used 剪枝互斥
  
Step 2: 对 α/β/γ 三队分别调用 assign_team_shift()
  ├─ α队: 绑定 trade_1 + 2个制造站 + 1个发电(共用替补) + 中枢(八幡海铃)
  ├─ β队: 绑定 trade_2 + 1个制造站 + 1个发电(共用替补) + 中枢(灵知)
  └─ γ队: 绑定 trade_3 + 1个制造站 + 1个发电(共用替补) + 中枢(怪猎)

Step 3: 组装 3 个班次
  ├─ shift0(12h): α.assignment ∪ β.assignment (γ 休息)
  ├─ shift1(6h):  β.assignment ∪ γ.assignment (α 休息)
  └─ shift2(6h):  γ.assignment ∪ α.assignment (β 休息)

Step 4: 对各班次评分（按不同 shift_hours）
```

### 3.2 新增类型

```rust
/// 三队标签
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamLabel {
    Alpha, // 但书/叙拉古链
    Beta,  // 灵知/喀兰贸易
    Gamma, // 怪猎/木天蓼
}

/// 绑定一个队的站索引
pub struct TeamStationBinding {
    pub label: TeamLabel,
    pub trade_station_idx: usize,       // 绑定贸易站
    pub manufacture_station_idxs: Vec<usize>, // 绑定制造站
    pub power_station_idxs: Vec<usize>,       // 绑定发电站
    pub control_bound_names: Vec<String>,     // 体系绑定干员
}

/// 各队编制
pub struct TeamAssignment {
    pub label: TeamLabel,
    pub assignment: BaseAssignment,  // α 队独占的房间
    pub operators: Vec<String>,
    pub scores: ShiftScores,
}

/// 输出报告
pub struct TeamRotationReport {
    pub shifts: Vec<TeamShiftResult>,
    pub teams: Vec<TeamAssignment>,
    pub total_weighted_score: f64,  // 12h×αβ + 6h×βγ + 6h×γα
    pub elapsed: Duration,
}

pub struct TeamShiftResult {
    pub index: usize,
    pub duration_hours: f64,
    pub active_teams: Vec<TeamLabel>,
    pub assignment: BaseAssignment,
    pub scores: ShiftScores,
}
```

### 3.3 `layout/assign.rs` 新增函数

```rust
/// 为某个队生成单队编制（只占该队绑定的房间）
pub fn assign_team(
    blueprint: &BaseBlueprint,
    pool_union: &TeamPools,  // 全量pool但受 used 剪枝
    instances: &OperatorInstances,
    table: &SkillTable,
    options: &AssignBaseOptions,
    binding: &TeamStationBinding,
    used: &mut HashSet<String>,  // 跨队互斥
) -> Result<TeamAssignment>
```

内部调用现有函数但限制 `room_id`：
- `assign_trade_meta` → 不搜全站，只搜绑定站（用 `must_include` 或站索引过滤）
- `assign_manufacture_lines` → 只填绑定的制造站
- `assign_power_stations` → 只填绑定的发电站（共用替补通过 used 实现）
- `assign_control` → 给定 `must_include` 列表（体系绑定干员）

---

## 4. 评分模型

### 4.1 当前评分

`score_base_assignment()` 对 `BaseAssignment` 一次评分，只返回 `effective_eff_multiplier`（纯效率乘数，不含时间权重）。

### 4.2 新评分需求

```
每日总产出 = 12h × shift0_eff + 6h × shift1_eff + 6h × shift2_eff
```

具体：
```rust
/// 按班次时长加权评分
fn score_team_shift(
    blueprint: &BaseBlueprint,
    assignment: &BaseAssignment,
    instances: &OperatorInstances,
    table: &SkillTable,
    shift_hours: f64,    // 12.0 or 6.0
    durin_plan: Option<u8>,
) -> Result<f64>
```

重用现有 `score_base_assignment`，但需传递 `shift_hours` 到 `TradeRoomInput` / `ManuRoomInput`。

### 4.3 当前分数模型存在的问题

当前 `score_base_assignment` 硬编码 `mood: 24.0`。12h 班和 6h 班的 `shift_hours` 不同，但当前 `TradeSearchOptions` 的 `shift_hours` 字段**只影响产出公式**（`eff × (shift/24) × 单位产出`），不影响效率本身 ✅

所以评分层面的改动很小：只需把 `shift_hours` 传入评分函数。

---

## 5. 修改量评估

| 模块 | 改动类型 | 估计行数 |
|------|---------|---------|
| `schedule/team_rotation.rs` | **新增** ~200行 | +200 |
| `layout/assign.rs` | 修改 + 新增函数 ~100行 | +100 |
| `layout/shift.rs` | 修改 enum | +3 |
| `schedule/mod.rs` | 新增 re-export | +5 |
| `schedule/base_rotation.rs` | 评分函数加 `shift_hours` 参数 | +10 |
| **合计净增** | | **~320行** |

**结论：改动量小而集中，现有模型非常适配。**

---

## 6. 关键决策点

### 6.1 三队搜索方式

**方案 A（推荐）**：三队逐个顺序搜索，用 `used` 剪枝互斥
```
α 队从全量池搜 → used += α.operators
β 队从 filter(pool, used) 搜 → used += β.operators
γ 队从 filter(pool, used) 搜
```

- 优点：实现简单，和现有 `assign_shift` 的 `used` 剪枝模式完全一致
- 缺点：α 队先选会占用最优干员，β/γ 后选受限
- 缓解办法：对 3 个队的 `search_trade_triples` 搜各自绑定的站 + `top_k` 缓存

**方案 B（更优）**：三队并行搜索各自的绑定站，然后 union `used` 消歧
```
α 队搜 trade_1 的 C(n,3), top_k=20
β 队搜 trade_2 的 C(n,3), top_k=20
γ 队搜 trade_3 的 C(n,3), top_k=20
→ pick_disjoint(α.top, β.top, γ.top, used)
```
- 优点：三队公平竞争，没有先后偏差
- 缺点：需要跨队联合消歧逻辑

### 6.2 发电站 5 人方案

- 不绑定特定队，而是 3+2=5 人轮换全局 pool
- 每班 3 个发电站全上，不区分 αβγ
- **只需在 `assign_power_stations` 增加一个 5 人轮换模式**

### 6.3 不同班次时长（12h vs 6h）

现有 `assign_shift` 接受 `options.shift_hours`。只需在调用时传不同值：
```rust
// 班次0: α+β, 12h
let shift0_assignment = merge(&alpha.assignment, &beta.assignment);
let shift0_score = score_base_assignment(blueprint, &shift0_assignment, ..., 12.0);

// 班次1: β+γ, 6h
let shift1_assignment = merge(&beta.assignment, &gamma.assignment);
let shift1_score = score_base_assignment(blueprint, &shift1_assignment, ..., 6.0);
```

---

## 7. 最终结论

| 问题 | 答案 |
|------|------|
| 现有模型能适配 αβγ 方案吗？ | **✅ 能，改动量小** |
| 核心改动在哪？ | `schedule/team_rotation.rs`（新房）+ `layout/assign.rs`（新增 team 模式） |
| 现有组件有多少可以直接复用？ | **~90%**（pool/search/solve/layout 全部复用） |
| 主要风险是什么？ | 三队搜索公平性（方案A 有先后偏差，方案B 更优） |
