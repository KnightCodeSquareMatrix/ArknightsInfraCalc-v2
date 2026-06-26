# 代码化体系编排层设计 plan

> 状态：ready
> 来源：[ADR 0001](../ADR/0001-layout-assignment-decomposition.md)（决策 B–D）、用户体系级架构诊断报告（2026-06-26）
> 关联：[SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md](SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md)、[SYSTEM_REGISTRY_NORMALIZATION_REPORT.md](SYSTEM_REGISTRY_NORMALIZATION_REPORT.md)、[../公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md](../公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md)
> 性质：接口与阶段设计文档，**不下沉到最终代码**。文中 Rust 片段为目标接口意图与现状陈述，标注来源 file:line；具体实现属另一轮。

## 与现有 plan 的分工（先读这段，避免重复）

- **本文** = ADR 0001 决策 B–D 的接口/阶段细节：代码化体系层接口、统一中间语义、execute 三态、anchor fill 阶段性候选池、两路径汇合。
- **`SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md`** = 数据驱动 registry 子集如何用 `base_systems.json` schema 表达 anchor / min_pick / constraint。
- **`SYSTEM_REGISTRY_NORMALIZATION_REPORT.md`** = registry / trade role / global resource 的分层审计与去 fixed 错位。

判据：能用声明静态表达（无运行期降级/替代/priority-by-tier）的体系走数据驱动 registry；带降级决策树的复杂体系走代码化体系层。两者产出同一套中间语义，汇合到统一 `AssignmentPlan`。

## 1. 目标与边界

### 目标
把“一个体系启动后产生的 anchor / producer / constraint / degradation”在 plan 层完整表达，统一被 execute + fill 消费，消除以下反向补救：

- `trade pool missing must-include operator`（`search/trade.rs:260`）这类阶段性候选池缺失导致的硬报错。
- `inject_search_anchor_pool_entries`（`layout/assign.rs:140-166`）在 pool 建好后再注入 anchor 的补丁。
- fill 时从 `used` 临时移除 anchor 再注入 pool（`trade_fill.rs:287-291`、`manufacture_fill.rs:246-248`）。
- commit 允许同房重复、扫描非空房判断 anchor 房（`commit.rs`、partial-room 分支）。

### 明确不做（报告第十三节）
- 不重写 L1/L2 机制求解器（贸易/制造底层公式）。
- 不改 scoring policy / 分量纲。
- 不让 CLI 承载体系逻辑。
- 不改 MAA export（消费最终排班即可）。
- 不在本计划内调数值锚点（迷迭香 75/90 等数值是数据/回归任务）。
- 不引入全局联合最优 / 整数规划 / `C(n,3)^站数`。
- 不在体系层重算效率（仍由 L1/L2/L3 + `resolve_base` / `cross_facility`）。

## 2. 统一中间语义（一等公民，接口设计非最终代码）

蓝本取自当前**死代码** `layout/system_integrity/plan.rs`，接线后转为生产路径。

```rust
// 体系锚点：钉核心干员与设施，队友由 fill 阶段补齐。蓝本 plan.rs:30。
pub struct SystemAnchor {
    pub system_id: String,
    pub operator: String,
    pub elite: u8,
    pub facility: FacilityKind,
    pub room_id: Option<RoomId>,      // None = 该设施类型首个空房，不绑 trade_1/manu_4
    pub fill_policy: AnchorFillPolicy, // trade role / manufacture recipe / plain
    pub constraints: Vec<SystemConstraint>,
}

// 只提供全局/跨设施资源的 producer。蓝本 OptionalProducer，plan.rs:40。
pub struct ProducerSlot {
    pub system_id: String,
    pub operator: String,
    pub elite: u8,
    pub facility: FacilityKind,
    pub optional: bool,               // 缺人时裁剪，不拖死核心
}

// 不占房但影响补齐搜索的约束。补 pairwise；现仅有系统级 exclusive_group。
pub enum SystemConstraint {
    ForbidSameRoom(String, String),     // 迷迭香 ↔ 清流+温蒂
    ForbidSameStation(String, String),  // 黑键 ↔ 巫恋
    RequireFacility(FacilityKind),
}

// 降级阶梯结果。蓝本 RosemaryTier + producers_present/missing，plan.rs:8-57。
pub struct DegradationLadder {
    pub system_id: String,
    pub tier_label: String,           // 满配 / 档1 / 档2 / 档3 / 替代感知源
    pub priority: i32,                // priority-by-tier：高档 21，档3 降至 15
    pub producers_present: Vec<String>,
    pub producers_missing: Vec<String>,
}

// 同班绑定，已存在于 plan.rs:21。
pub struct ShiftBind {
    pub operators: Vec<String>,       // 迷迭香 + 黑键
    pub on_shifts: u8,                // 上 2
    pub off_shifts: u8,               // 休 1
}
```

`AssignmentPlan` 在现有 `{mode, activated, registry_claims}`（`orchestrate/plan.rs:43-50`）基础上新增、并保留 `registry_claims` 兼容：

```rust
pub struct AssignmentPlan {
    pub mode: AssignShiftMode,
    pub activated: Vec<ActivatedSystem>,
    pub registry_claims: Vec<RegistrySystemClaim>,   // 保留：数据驱动 registry 落位
    // 新增：两路径汇合后的统一体系语义
    pub anchors: Vec<SystemAnchor>,
    pub producers: Vec<ProducerSlot>,
    pub constraints: Vec<SystemConstraint>,
    pub degradations: Vec<DegradationLadder>,
    pub shift_binds: Vec<ShiftBind>,
}
```

## 3. 体系层接口

```rust
// 输入：已存在于 system_integrity/context.rs（EvaluateContext{blueprint, operbox, mode}）。
pub trait SystemEvaluator {
    fn evaluate(&self, ctx: &EvaluateContext<'_>) -> SystemVerdict;
}

pub enum SystemVerdict {
    Activate(SystemPlanFragment),    // anchors + producers + constraints + degradation + shift_bind
    Skip(SkipReason),                // 蓝本 SkipReason，plan.rs:60-72
}
```

接线：

- 迷迭香 evaluator 接现有 `evaluate_rosemary`（`system_integrity/rosemary.rs:24`），其四档 `RosemaryTier` + 感知源替代 + layout gate 直接复用。
- 聚合入口 `evaluate_systems`（`system_integrity/mod.rs:20`，当前零生产调用）转为生产调用，结果并入 `build_plan`（`orchestrate/select.rs`）。
- 后续红松林 / 自动化 / 深巡乌尔比安各实现一个 evaluator，注册进聚合入口。
- 两路径汇合仲裁：体系层先认领 → 把已占 system_id / 房间反馈给数据驱动 registry 的 `skip_system_ids`（现有机制，`assign.rs:118-119`、`265-267`、`trade_fill.rs:204-207`），registry 在体系层已占资源上 skip，避免重复认领。

## 4. execute 三态（数据流）

取代当前 `execute_plan` 对所有 claim 无差别 `set_room` + `used.insert`（`execute.rs:34-40`、`system.rs:801-830`）。

| 状态 | 触发阶段 | `assignment` | `used` | 房间满员 | 复用现有 |
|------|----------|--------------|--------|----------|----------|
| reserved | execute_plan | 只放 anchor 核心 | 核心计入 | 否（可未满 3） | `set_room`（`assignment.rs:162`） |
| required | anchor fill | 不变（搜索进行中） | 不变 | 搜索补齐中 | `SearchTripleFilter.must_include_name`（`trade.rs:147`）/ `ManuSearchOptions.must_include_name`（`manufacture.rs:83`） |
| committed | commit | 写满 3 人 | 全员计入 | 是 | `commit_anchor_room`（`commit.rs:13`）/ `commit_operators_to_room`（`commit.rs:168`）的 anchor 豁免 |

关键：`execute_plan` 按 anchor/`SlotFillMode` 分支。`fill:"search"` 槽 → reserved（不补满、队友不计 used）；`Fixed` 槽 → 直接 committed。

## 5. fill 阶段边界 + 阶段性候选池

### 目标流水线（对齐 ADR 0001 Pipeline 阶段门）

```text
seed/pinned
  -> build_plan          代码化体系层 + 数据驱动 registry 汇合，产出统一 plan
  -> execute             reserved：落核心，不补满
  -> producer placement  统一 ProducerSlot（取代 producer_fill 手写 夕/絮雨/爱丽丝/车尔尼/乌尔比安/森西）
  -> resolve global      resolve_base 快照
  -> anchor trade fill    required → 阶段性候选池 + must_include + forbid_with；role/L3 shortcut 优先
  -> anchor manu fill     required → 阶段性候选池 + must_include + min_pick + forbid_with
  -> plain trade / manu   剩余房贪心
  -> final resolve / score
```

### 阶段性候选池（消除报告第五节缺口）

当前 pool 是设施视角（`build_trade_pool` 纯 roster/facility/skill 过滤，不感知体系 anchor），导致 anchor 可能不在池中。目标：`required` 阶段的 pool 按“**已启动体系 + 当前 box**”构造，使 anchor 必在池中——把 `inject_search_anchor_pool_entries` 的补丁逻辑收编为 pool 构造的正式输入。

### 制造补齐与贸易对称

当前制造仅有 plain recipe + standalone + capacity fallback + 公孙金线硬编码（`manufacture_fill.rs`），缺贸易已有的接口。需补：

| 能力 | 贸易现状 | 制造目标 |
|------|----------|----------|
| `must_include` | 有（`SearchTripleFilter`） | 补真正的 plan-driven anchor（现仅 re-pin `existing[0]`） |
| `min_pick` | — | 补（灰毫/远牙/野鬃至少 2） |
| `forbid_with` | 有（`trade_station_exclusive_violation`、黑键/巫恋 greedy filter） | 补（迷迭香 ↔ 清流+温蒂） |
| recipe-bound anchor | n/a | 补（赤金线锚定） |
| optional line | — | 补（红松砾赤金可选线缺失不拖死经验核心） |

### trade role 重定位

`role_pick.rs`（docus/closure/witch）重定位为 **anchor fill policy**，不再是独立于 registry 的体系选择器（呼应报告第八节、registry 报告 §3.4）。即：体系/registry 决定“黑键贸易 anchor 启动”，role policy 决定“如何补齐这个 anchor 房”。

## 6. 迁移顺序（对齐 ADR 0001 Phase 2–7）

| Phase | 动作 | 验收 |
|-------|------|------|
| 2 语义骨架 | `orchestrate/plan.rs` 增 §2 类型 + 序列化 | 不改排班结果；现有测试全绿 |
| 3 体系层接线 | `evaluate_systems` 接入 `build_plan`，迷迭香走代码化路径；`rosemary_perception` 降级 | full-E2 243 迷迭香+黑键落位不退化 |
| 4 execute 三态 | execute 按状态分支，reserved 不补满；anchor fill 阶段性候选池 | 不再出现 must-include 报错；anchor 参与三人搜索 |
| 5 制造对称接口 | manufacture_fill 补 must_include/min_pick/forbid_with/optional line；迁红松林、自动化 | 红松经验线核心不因砾赤金可选缺失关闭 |
| 6 producer 统一 | producer_fill 手写 → ProducerSlot；深巡乌尔比安宿舍 anchor 纳入 | producer 缺失作为降级输出，explain 可解释 |
| 7 轮换接入 | team_rotation 消费 plan 的 anchor/producer/degradation/shift_bind | γ 半区复用同一套 anchor fill，不再房间名反推 |

每阶段单独验收、行为可解释。Phase 1（facade 机械拆分）见 ADR 0001，是本序列的基底。

## 7. 回归矩阵

沿用 `SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md` 回归矩阵 + ROSEMARY chain §7 数值锚点。

- [ ] `cargo test -p infra-core --quiet`
- [ ] `cargo run -q -p infra-cli -- verify --all`
- [ ] `cargo run -q -p infra-cli -- plan --operbox data/fixtures/243/operbox_full_e2.json --maa-out out/243_maa.json`
- [ ] full-E2 243：但书、黑键、迷迭香、自动化金线、红松经验线均落位。
- [ ] 缺迷迭香或黑键：感知链关闭，普通贪心接管，explain 给原因。
- [ ] 缺絮雨但有八幡海铃/焰狐龙梓兰：降级到替代感知源档（chain §8.2 / 附录 B，感知最低约 40），不误报核心缺失。
- [ ] 缺一名红松制造干员：红松经验线仍可搜第三人补齐。
- [ ] 无薇薇安娜：红松林核心关闭。
- [ ] 黑键不会与巫恋同贸易站；清流+温蒂不会与迷迭香同制造站。
- [ ] 轮换中迷迭香+黑键同上同下、上 2 休 1。
- [ ] 迷迭香效率落在 75/80/90 档（chain §7），缺宿舍 ~55。

## 8. 完成后归档

完成后移动到 `docs/ARCHIVE/done/`，并更新：

- [ORCHESTRATION_LAYER.md](../ORCHESTRATION_LAYER.md)：System schema 与两路径汇合事实。
- [BASE_ASSIGNMENT.md](../BASE_ASSIGNMENT.md)：anchor 三态落位顺序与职责边界。
- [SCHEDULE_ROTATION.md](../SCHEDULE_ROTATION.md)：shift_bind 与 anchor 在轮换中的关系。
- [SYSTEM_CHAINS.md](../SYSTEM_CHAINS.md)：代码化体系层覆盖的体系清单。
