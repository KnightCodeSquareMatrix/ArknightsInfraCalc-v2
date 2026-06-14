# αβγ 三队轮换 — 实现计划（可执行）

> 前置：`plans/team_rotation_design.md`（方案）、`plans/feasibility_analysis.md`（可行性）。
> 本文是落地工序，已锁定两项决策，并修正了可行性文档里被高估/低估/自相矛盾的点。

---

## 0. 已锁定决策

1. **发电产出计入评分**：每日总分 = 贸易 + 制造 + **发电** 三类，均按班次时长加权。
2. **走 `assign_shift` 加站绑定参数**：不暴露私有 `assign_*` helper、不另写 `assign_team`；
   通过给 `assign_shift` 增加一个「站白名单 + 体系绑定」的入参，让同一流水线只填某队绑定的房间。
3. 三队搜索先用**方案 A（贪心 + `used` 跨队互斥）**，方案 B（并行 top-k 跨队消歧）留作后续优化。
4. 心情自洽**仅留在设计文档**，求解器/报告不计算心情（遵守 AGENTS.md §8 非目标）。
5. 旧 `schedule_base_rotation_a_b_a` 保留为 fallback。

---

## 1. 评分公式（先定死，再写代码）

现状 `score_base_assignment`（`schedule/base_rotation.rs:66`）：
- trade 累加 `solve_trade_with_shift(...).effective_eff_multiplier`（纯效率乘数）
- manu 累加 `solve_manufacture(...).prod_total`
- **无发电**；`mood` 与 `shift_hours` 均硬编码 24.0。

新公式（每班产出 ∝ 效率 × 时长）：

```
shift_score(assignment, h) =
      w_trade * Σ trade_eff(room, h)
    + w_manu  * Σ manu_prod(room)
    + w_power * Σ power_charge_pct(room, h)

daily_total = Σ_{shift∈{12h,6h,6h}}  shift_score(shift.assignment, shift.h) * (h / 24)
```

要点（修正可行性文档 §4.2/§4.3 的自相矛盾）：
- `shift_hours` **不改** `effective_eff_multiplier`，所以**时间加权必须在汇总层显式 `× (h/24)`**，
  而不是「把 shift_hours 传进 solve 就行」。
- 发电用 `solve_power(...).charge_speed_pct` 累加（见 `power/solver.rs:63`）。
- `ResolvedPowerRoom { id, operator, layout }`（`layout/resolve.rs:38`）已含逐房 layout，
  评分时对每个 power room 用其 `operator + layout + shift_hours` 重跑 `solve_power`，与 trade/manu 同构。
- 权重 `w_trade/w_manu/w_power` 先各取 1.0，作为可调常量；单位不可比时仅用于「同方案不同班对比」，不跨类比较。

---

## 2. 数据结构（新增）

放在 `schedule/team_rotation.rs`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamLabel { Alpha, Beta, Gamma }

/// 每队绑定哪些房间 + 体系干员
pub struct TeamBinding {
    pub label: TeamLabel,
    pub trade_room: RoomId,                 // 该队 1 个贸易站
    pub trade_anchor: Option<TradeAnchor>,  // 但书 / 巫恋 / 可露 / 灵知 / 怪猎；None=纯贪心
    pub manufacture_rooms: Vec<RoomId>,     // α=2 站, β=1, γ=1
    pub control_bound: Vec<String>,         // 体系绑定中枢干员（八幡海铃/灵知/怪猎…）
    pub dorm_room: Option<RoomId>,          // 跟队宿舍 producer（可选）
}

pub enum TradeAnchor { Docus, Witch, Closure, Karlan, Matatabi } // 复用现有 shortcut 过滤器

pub struct TeamAssignment {
    pub label: TeamLabel,
    pub assignment: BaseAssignment,  // 仅该队占用的房间
    pub operators: Vec<String>,
}

pub struct TeamShiftResult {
    pub index: usize,
    pub duration_hours: f64,         // 12.0 / 6.0 / 6.0
    pub active_teams: Vec<TeamLabel>,
    pub assignment: BaseAssignment,  // 两队 merge 后的整基建编制
    pub scores: ShiftScores,         // 复用，新增 power 字段
    pub weighted_score: f64,         // scores 折算 × h/24
}

pub struct TeamRotationReport {
    pub teams: Vec<TeamAssignment>,
    pub shifts: Vec<TeamShiftResult>, // 3 班
    pub daily_total: f64,
    pub elapsed: Duration,
}
```

`ShiftScores`（`base_rotation.rs:28`）扩展：

```rust
pub struct ShiftScores {
    pub trade_score: f64,
    pub manu_prod_sum: f64,
    pub power_charge_sum: f64,  // 新增
}
```

> 注意：`ShiftScores` 被旧 ABA 复用，加字段后旧路径需补默认/计算（见 §3 步骤 1）。

---

## 3. 工序（分 6 步，每步带验证门）

### 步骤 1 — 评分加发电 + 时间加权（最先做，独立可测）
- 文件：`schedule/base_rotation.rs`
- 改 `ShiftScores` 增 `power_charge_sum`。
- 改 `score_base_assignment` 增 `shift_hours: f64` 参数，并：
  - trade：`solve_trade_with_shift(&input, table, shift_hours)`（input.mood 仍 24.0 纸面）。
  - power：遍历 `resolved.power_rooms`，逐房 `solve_power` 累加 `charge_speed_pct`。
- 新增汇总 helper：
  ```rust
  pub fn weighted_shift_score(scores: &ShiftScores, hours: f64) -> f64 {
      (scores.trade_score + scores.manu_prod_sum + scores.power_charge_sum) * (hours / 24.0)
  }
  ```
- 旧 `schedule_base_rotation_a_b_a` 调用处补传 `24.0`，保持行为不变。
- **验证门**：`cargo test -p infra-core schedule::base_rotation` 全绿（旧两条断言不回归）。

### 步骤 2 — `assign_shift` 站绑定参数（核心）
- 文件：`layout/assign.rs`、`layout/shift.rs`
- 新增（`shift.rs`）：
  ```rust
  #[derive(Debug, Clone, Default)]
  pub struct StationBinding {
      pub trade_rooms: Option<HashSet<RoomId>>,        // None=全部空房（旧行为）
      pub manufacture_rooms: Option<HashSet<RoomId>>,
      pub power_rooms: Option<HashSet<RoomId>>,
      pub control_must_include: HashSet<String>,       // 体系绑定干员
      pub trade_anchor: Option<TradeAnchor>,
  }
  ```
- `assign_shift` 签名增 `binding: &StationBinding`（旧调用传 `&StationBinding::default()`）。
- 内层各 `assign_*_lines` 增「房间是否允许」判断：
  ```rust
  fn room_allowed(filter: &Option<HashSet<RoomId>>, id: &RoomId) -> bool {
      filter.as_ref().map_or(true, |s| s.contains(id))
  }
  ```
  在 `assign_trade_meta` / `assign_trade_remainder` / `assign_manufacture_lines` /
  `assign_power_stations` 的房间循环里加 `if !room_allowed(...) { continue; }`。
- `assign_control`：把 `binding.control_must_include` 并入现有 `pinned`/`must_include`（机制**已存在**，
  见 `assign.rs:294`，只需把外部传入的体系干员塞进 `must_include`）。
- **单贸易站 + anchor**：当 `trade_rooms` 仅含 1 房且 `trade_anchor=Some(...)` 时，
  该房用对应 shortcut 过滤器（`pick_docus_trade_hit` / `hit_witch_shortcut` / `hit_closure_shortcut`），
  失败回退 Plain 贪心（复用 `trade_rotation.rs:68 pick_station` 的回退范式）。
- **验证门**：新增单测——给定一个只绑 `trade_2`+`manu_3` 的 binding，`assign_shift` 只填这两房，其余空。

### 步骤 3 — 发电：直接复用现有贪心，不做轮换池（已简化）
- **依据**：看 `skill_table.json` 的 power 原子——绝大多数发电技能都是 `add_flat_eff value:10`
  （纯 +10% 充能），彼此可互换；特殊位仅技术交流·α/β（空构爬升）、晨曦（虚拟发电站）、
  巡线框架/生态科主任/灵河共鸣（按 drone/莱茵/宿舍等级缩放）、几个 +5% 条件挂件。
  → **发电是「每站 1 人、纯加法、站间几乎无耦合」**，`search_power_top`（按 `charge_speed_pct`
  降序逐站取最优可用，`search/power.rs:111`）已接近最优；+10% 填充位在大盒里永远够。
- 设计文档「3+2=5 人替补池」是**压练度需求**的产物；本仓库不压练度，**砍掉该抽象**。
- 落地：每班直接调现有 `assign_power_stations`（`assign.rs:687`），传该班 `shift_hours`
  + 该班 `used`，贪心填满 3 站即可。
  - 空构爬升位 `charge_speed_pct` 随 `shift_hours` 变高，按正确 shift_hours 评分时贪心**自然**
    把爬升位优先放到 12h 长班。
  - 发电不绑 αβγ（设计 §3.2 已确认独立），跟 §4 整班装配一起做，本步无独立产物。
- **本步无新代码**，仅在步骤 1 把 power 纳入评分（已覆盖）；删除原「5 人轮换池」工作量。

### 步骤 4 — `schedule/team_rotation.rs` 主逻辑（新房）
- 文件：`schedule/team_rotation.rs`（新建）、`schedule/mod.rs`（re-export）
- 入口：
  ```rust
  pub fn schedule_team_rotation(
      blueprint: &BaseBlueprint,
      operbox: &OperBox,
      instances: &OperatorInstances,
      table: &SkillTable,
      options: &AssignBaseOptions,
  ) -> Result<TeamRotationReport>
  ```
- 流程：
  1. 推导 243 蓝图的 αβγ `TeamBinding`（trade_1/2/3 各一队；manu α=2 站、β/γ 各 1；体系干员按设计 §4 表）。
     - 蓝图站不足/不是 243 时：报错或退化（先支持 243，其它蓝图列为 §5 待办）。
  2. **顺序**对 α→β→γ 调 `assign_shift(..., binding_i, ...)`，**共享一个 `used`** 实现跨队互斥
     （方案 A）。每队产出 `TeamAssignment`（仅自己的房间）。
  3. 发电用步骤 3 的 5 人轮换池，得到每班发电编制。
  4. 组装 3 班整基建编制（merge 两队 trade/manu + 该班发电 + 钉死的中枢/宿舍）：
     - shift0(12h): α∪β（γ 休）
     - shift1(6h):  β∪γ（α 休）
     - shift2(6h):  γ∪α（β 休）
  5. 逐班 `score_base_assignment(..., shift_hours)` + `weighted_shift_score`，汇总 `daily_total`。
  6. 跨班互斥断言（沿用 `assert_disjoint` 范式）：同一班两队 trade/manu/power 干员不重合。
- **验证门**：单测——243 + `operbox_full_e2.json`，3 班生成、各班无重复、`daily_total>0`、
  α/β/γ 各上 2 班休 1 班。

### 步骤 5 — CLI + 输出（可行性文档遗漏的 scope）
- 文件：`infra-cli`（子命令/flag）、`infra-cli/.../output.rs`（`emit_team_rotation`）。
- 新增 `layout team-rotation`（或在 `layout test` 下加 `--team-rotation`）子命令，
  入参沿用标准夹具（243 + `operbox_full_e2.json`），`--text` / `--json` 输出。
- 输出：3 班矩阵（队×班 上/休）、各班三类分、`daily_total`、各队干员清单。
- **验证门**：
  ```
  cargo run -p infra-cli -- layout test \
    --layout data/fixtures/243/layout.json \
    --operbox data/fixtures/243/operbox_full_e2.json --team-rotation --text
  ```
  人工确认 αβγ 节奏与体系绑定正确。

### 步骤 6 — 回归夹具 + 收口
- 加一个 `team_rotation` 回归锚（参照 `infra-cli/src/verify/fixtures.rs` 现有范式）。
- 跑全套：
  ```
  python scripts/build_skill_table.py
  cargo test -p infra-core
  cargo run -p infra-cli -- verify --all
  ```
- 文档回写：`docs/PROJECT_MAP.md`（新模块路由）、`docs/BASE_ASSIGNMENT.md`（αβγ 模式说明）。

---

## 4. 改动清单与量级（修正可行性文档的 ~320 行）

| 模块 | 改动 | 估计 |
|------|------|------|
| `schedule/base_rotation.rs` | `ShiftScores` 加 power、`score_base_assignment` 加 `shift_hours`、加权 helper | +40 |
| `layout/shift.rs` | `StationBinding` / `TradeAnchor` | +40 |
| `layout/assign.rs` | `assign_shift` 加 binding 参数、各 `assign_*` 加房间白名单、anchor 单站逻辑 | +120 |
| 发电 | **复用现有贪心，无新代码**（仅评分纳入，已含在步骤 1） | 0 |
| `schedule/team_rotation.rs` | **新建**：binding 推导 + 三队装配 + 三班评分 | +220 |
| `schedule/mod.rs` | re-export | +5 |
| `infra-cli` + `output.rs` | 子命令 + emit | +120 |
| 测试 + 夹具 | 单测 + 回归锚 | +120 |
| **合计** | | **~665 行** |

> 比文档 ~320 行翻倍主因：步骤 2（站白名单贯穿流水线）+ 步骤 5（CLI/输出，文档遗漏）。
> 发电按你指正已简化为零新代码。

---

## 5. 风险与待办

1. **三队公平性**：方案 A 有先后偏差（α 先抢最优）。181/114 人池下竞争低，先接受；
   若 β/γ 明显劣化，再上方案 B（各站 top-k 并行 + 跨队 disjoint pick，需新写联合选择）。
2. **单站 anchor 映射**：但书/巫恋/可露原是「全基建 3 站 meta 级联」，拆成「每队 1 站 anchor」后，
   组合最优可能略低于一次性 3 站级联——可接受（轮换体系本就牺牲峰值换利用率），但需在报告里说明。
3. **灵知/喀兰跨设施**：β 队绑灵知时，喀兰精度注入是跨设施 manifest（AGENTS.md：`global_resource/inject.rs`），
   要确认 β 队的中枢 binding 与其贸易站同班，否则注入落空。
4. **怪猎木天蓼**：γ 队中枢需 consumer 同班（`assign.rs:1055` 单测已示范 seed consumer），binding 要保证 γ 的 trade/制造里有 consumer 同班。
5. **非 243 蓝图**（252 等）：本期只做 243，binding 推导对其它蓝图报错；列为后续。
6. **权重不可比**：trade 效率乘数 / manu 产量 / power 充能% 单位不同，`daily_total` 只用于**同蓝图不同排班对比**，不对外当绝对产量。

---

## 6. 执行顺序总结

步骤 1（评分加发电+加权，独立可测）→ 步骤 2（站绑定，核心）→ 步骤 3（发电：复用贪心，无新代码）
→ 步骤 4（三队主逻辑）→ 步骤 5（CLI/输出）→ 步骤 6（回归+文档）。

每步都有验证门，步骤 1/2 互相独立、可分别合入，降低大改写风险。
发电按技能表特性已确认无需新机制（步骤 3 退化为评分纳入）。

---

## 7. 实现落地记录（v1 已完成）

已按上述计划落地并跑通 3 班排班表（`cargo test -p infra-core` 223 passed）。

**实际改动文件**：
- `schedule/base_rotation.rs`：`ShiftScores` 加 `power_charge_sum` + `weighted(h)`；
  `score_base_assignment` 加 `shift_hours` 参数并对每个 `ResolvedPowerRoom` 跑 `solve_power` 累加充能%。
- `layout/assign.rs`：新增 `assign_team_producer_rooms`（站绑定，复用同模块私有 `pick_trade_hit`/
  `pick_manu_hit` 等）；`assign_power_stations` 改 `pub`。
- `layout/mod.rs`、`schedule/mod.rs`、`lib.rs`：导出新符号。
- `schedule/team_rotation.rs`（新建）：`schedule_team_rotation` + `TeamLabel/TeamAssignment/
  TeamShiftResult/TeamRotationReport`。
- `infra-cli`：`layout team-rotation` 子命令 + `emit_team_rotation`（text/csv/json）。

**与原计划的偏差（已确认合理）**：
1. **站绑定走新 helper 而非给 `assign_shift` 加参数**：`assign_team_producer_rooms` 在 `assign.rs`
   同模块内直接复用私有搜索 helper，比把白名单贯穿 `assign_shift` 整条流水线更干净、风险更低。
2. **真实 243 是 2 贸易站**（非设计文档假设的 3）：改为「贸易/制造房间按出现顺序 round-robin 分给
   αβγ」的通用切分。结果 α=trade_1+manu_1+manu_4、β=trade_2+manu_2、γ=manu_3——γ 偏小，
   导致 shift2(β+γ) 较弱。这是 2÷3 不整除的固有不均衡，v1 接受。
3. **中枢/宿舍/发电 v1 三班钉死**（共享脚手架，不单独轮换）：发电充裕、心情非目标，简化合理；
   设计文档「中枢最多 5 人轮休」「发电 3+2 替补」留作后续。
4. **trade anchor 未显式绑定**：每队贸易站取可用最优三人组（shortcut 自然高分），实测
   12h 班已自动落位 巫恋/龙舌兰（trade_1）+ 可露希尔（trade_2）+ 八幡海铃（中枢）。

**实跑结果**（243 + `operbox_full_e2.json`，owned=418）：
```
shift1 12h α+β  trade=6.812 manu=380.0 power=60.0  weighted=223.406
shift2  6h β+γ  trade=3.337 manu=217.0 power=60.0  weighted= 70.084
shift3  6h γ+α  trade=3.475 manu=375.0 power=60.0  weighted=109.619
daily_total = 403.109
```

**后续可选优化**：γ 队偏小的再平衡（按分数而非顺序切分房间）、中枢/发电跨班轮休、
per-team trade anchor 显式绑定（α 但书链 / β 灵知喀兰 / γ 怪猎）、252 等其它蓝图适配、回归夹具。
