# ADR 0001: layout 体系编排与 assignment 拆分

> 决策状态：accepted
> 实现状态：pending
> 日期：2026-06-26（初稿）／2026-06-26（融合代码化体系编排层）
> 关联文档：[../ORCHESTRATION_LAYER.md](../ORCHESTRATION_LAYER.md)、[../BASE_ASSIGNMENT.md](../BASE_ASSIGNMENT.md)、[../TODO/CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md](../TODO/CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md)、[../TODO/SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md](../TODO/SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md)、[../TODO/SYSTEM_REGISTRY_NORMALIZATION_REPORT.md](../TODO/SYSTEM_REGISTRY_NORMALIZATION_REPORT.md)、[../公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md](../公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md)

本 ADR 覆盖两个相互咬合的结构性决策：

- **A. assignment facade 拆分**：把 `layout/assign.rs` 收敛为薄 facade + 按职责命名的子模块。
- **B. 代码化体系编排层**：把体系启动后的 anchor / producer / constraint / degradation 升级为一等公民，让“数据驱动 registry”和“代码化体系层”两条入口汇合到统一 `AssignmentPlan`，由统一的 execute + fill 阶段消费。

`accepted` 表示这两个边界决策已接受，**不表示已实现**。具体接口与分阶段执行清单放在 `docs/TODO/CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md` 与 `docs/TODO/SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md`；本文只记录为什么这样拆、拆完后边界如何保持。

## 背景

### 症状一：assignment orchestration facade 过胖

`crates/infra-core/src/layout/assign.rs` 是全基建单班与轮换填房的事实入口。它已调用 `layout/orchestrate::{build_plan, execute_plan}`，但主流程和大量策略仍集中在一处：

- `assign_shift_with_plan_skip` 串联 seed、registry plan、producer、resolve、建池、发电、贸易、制造。
- 中枢补位、宿舍/办公室 producer、感知 producer、深巡/乌尔比安宿舍锚点都在同一层。
- 贸易核心优先通过 `pick_trade_meta_then_plain` 调 `search/role_pick.rs`，但仍保留 `skip_trade_core_registry_systems` 跳过旧 registry 抢站条目。
- 制造存在公孙金线固定锚点、候选池扩展、容量兜底等局部策略。
- 轮换半区填充函数也曾放在同一文件。

这不是机制层错乱。L1/L2/L3 求解、`resolve_base`、`search/*` 职责基本清楚。问题是**宏观流水线、设施填房 policy、producer policy、轮换填充 helper、提交/快照工具混在一起**，导致新增体系时容易继续把特例塞回 `assign.rs`。

### 症状二：plan 层没把体系语义当一等公民

更深一层的问题是 plan 层语义不足，已对照代码核实：

- `AssignmentPlan` 只有 `{mode, activated, registry_claims}`；`activated` 由 `registry_claims` 派生（`orchestrate/select.rs:29`）。
- 一个被认领的 slot 唯一的“状态”是 `RegistrySlotClaim.fill ∈ {Fixed, Search}`（`layout/system.rs:40-46`）。没有 anchor / producer / optional / reserved / required / committed 的状态区分。
- `execute_plan` **不区分 `SlotFillMode`**：对 `fill:"search"` 槽和 `Fixed` 槽一样，立即 `set_room` + `used.insert`（`orchestrate/execute.rs:34-40`、`layout/system.rs:801-830`）。于是“体系锚点”一旦认领就**既进房又占 used**，后续搜索又要把它拿出来参与三人组，只能在多处做反向补救（`inject_search_anchor_pool_entries`、临时从 `used` 移除再注入 pool、commit 允许同房重复、扫描非空房判断 anchor 房）。
- pool 是设施视角构造（`build_trade_pool` 等），不感知“已启动体系”。anchor 不在普通工具人 pool 里却必须参与搜索，于是出现 `trade pool missing must-include operator`（`search/trade.rs:260`）这类直接失败——缺的是“阶段性候选池”。
- `base_systems.json` / `SystemSlotDef` 的 schema 其实已支持 `optional`、`recipe`、`pick_one`、`fill:"search"`、`max_elite`、`exclusive_group`、`trade_role`。真正缺的是 `producer` / `anchor` 关键字、`forbid_with`（pairwise 禁配，现仅有系统级 `exclusive_group`）、显式降级阶梯。
- 体系入口分裂：registry（`base_systems.json`）、trade role（`trade_segments.json` / `role_pick.rs`）、`producer_fill.rs` 手写、以及**整个 `layout/system_integrity/` 模块**。其中 `system_integrity` 含 `RosemaryTier` 四档降级阶梯、`SystemAnchor`、`OptionalProducer`、`ShiftBind`、`SkipReason`，但**零生产调用**（仅被自身 `#[cfg(test)]` 触发）。迷迭香因此被双重实现：活路径是 `base_systems.json` 的扁平 `rosemary_perception`，死代码是 `system_integrity::rosemary` 的完整降级树。

### 判断

迷迭香不是异常个案，而是第一个同时需要“全局资源 producer + 贸易 consumer anchor + 制造 consumer anchor + 同班绑定 + 降级路径 + 非固定队友 + 禁配约束”的体系。它的降级是一棵**决策树**（感知源替代、priority-by-tier、layout gate，见 ROSEMARY chain 文档 §4 / §5.3 / 附录 B），用静态 JSON slot 列表表达成本高于收益。红松林、自动化、深巡乌尔比安继续推进会遇到同类问题。

因此正确边界是：**`layout/orchestrate` + 代码化体系层认领体系结构，`assign/*_fill.rs` 补齐 anchor 房，求解器只消费最终编制。** facade 拆分（决策 A）是基底，代码化体系层（决策 B）是落在其上的语义层；二者必须协调排序，不能互相 defer。

## 决策

保留现有公开入口，不做大重写，不引入全局联合最优。

### A. assignment facade 拆分

将 `layout/assign.rs` 收敛为薄 facade，内部实现拆为按职责命名的子模块：

```text
crates/infra-core/src/layout/assign/
  mod.rs              # public facade: assign_base_greedy / assign_shift / result/options
  pipeline.rs         # 单班流水线：阶段顺序、resolve 阶段门、fillers 调度
  run.rs              # AssignmentRun / FillContext：blueprint、options、used、durin、layout 快照
  commit.rs           # commit room、names_disjoint、efficiency snapshot
  control_fill.rs     # 中枢补位
  producer_fill.rs    # 宿舍/办公室/global producer 落位入口（后续统一为 ProducerSlot）
  trade_fill.rs       # 贸易余站、role policy、anchor 补齐、恢复班孑站
  manufacture_fill.rs # 制造产线、anchor 补齐、候选池、容量兜底
  power_fill.rs       # 发电站填充
  team_fill.rs        # αβγ 半区填充 helper
```

`run.rs` 避免与既有 `layout/context.rs` 撞名。`layout/orchestrate/` 保持为 registry plan 层，不吸收设施搜索逻辑：

```text
layout/orchestrate/
  plan / select / execute   # 只处理 System 选型与 fixed/anchor/producer 落位状态
```

### B. 代码化体系编排层

激活 `layout/system_integrity/`（实现期可更名 `layout/systems/`）为活路径，负责带运行期决策的复杂体系。形成**两条体系入口、一个汇合点**：

| 体系类型 | 入口 | 判据 |
|----------|------|------|
| 固定三人 / 简单 cross/same-station / pick_one | 数据驱动 registry（`base_systems.json` + `orchestrate/`） | 无运行期降级 / 替代 / priority-by-tier |
| 迷迭香感知链 | 代码化体系层 | 四档降级 + 感知源替代 + layout gate + priority-by-tier |
| 红松林 / 自动化 / 深巡乌尔比安 | 代码化体系层 | 核心 + 可选线分离、min_pick、producer-gated、宿舍 anchor |
| global resource 结算 | `resolve_base` / `cross_facility`（不变） | 体系层只决定 producer 是否上岗，效率仍由 resolve 算 |

两条入口都产出**同一套中间语义**（anchor / producer / constraint / degradation），写进**统一 `AssignmentPlan`**，由同一个 execute + 同一套 fill 阶段消费。

### C. anchor / producer / constraint / degradation 升级为一等公民

在 plan 层定义这四类语义（具体结构见 `CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md` §2），不再散落于 `producer_fill.rs` 手写、pool injection、partial-room 反向补救：

- `SystemAnchor`：钉核心干员与设施，队友由 fill 阶段补齐（蓝本 `system_integrity/plan.rs:30`）。
- `ProducerSlot`：只提供全局/跨设施资源（蓝本 `OptionalProducer`，`plan.rs:40`），统一 `producer_fill.rs` 的 夕/絮雨/爱丽丝/车尔尼/乌尔比安/森西 手写逻辑。
- `SystemConstraint`：`ForbidSameRoom` / `ForbidSameStation` / `RequireFacility`（补 pairwise 禁配，现仅有 `exclusive_group`）。
- `DegradationLadder`：已选档位 + 已裁剪 producer + 缺失原因（蓝本 `RosemaryTier` + `producers_present/missing`）。

### D. anchor 三态：execute 阶段必须区分 SlotFillMode

取代当前“claim 即 set_room + used”，引入三态：

- **reserved**：体系层认领核心。`set_room` 只放核心、不补满、队友不计 `used`；核心计 `used`。可留下未满 3 人的贸易/制造房。
- **required**：fill 阶段把 anchor 核心作为 `must_include` 传给搜索（复用 `SearchTripleFilter.must_include_name` / `ManuSearchOptions.must_include_name`），且 pool 在 fill 阶段按“已启动体系 + 当前 box”**阶段化构造**，使 anchor 必在池中——消除 `trade pool missing must-include operator` 报错与 `inject_search_anchor_pool_entries` 补丁。
- **committed**：搜索补满三人后统一 commit 写房 + used（复用 `commit_anchor_room` / `commit_operators_to_room` 的 anchor 豁免逻辑）。

明确：`execute_plan` 必须按 anchor 状态分支，不再对 search 槽无差别 `set_room` + `used.insert`。

### E. 死代码处置

`system_integrity` 不删除，作为代码化体系层的**原型蓝本**保留并接线（`evaluate_systems`，`system_integrity/mod.rs:20`，转为生产调用）。迷迭香迁为代码化路径后，`base_systems.json` 的 `rosemary_perception` 降级为“引用代码化体系”或移除。

## API 保留

拆分与体系层接线期间保持调用方稳定：

| 入口 | 可见性目标 | 备注 |
|------|------------|------|
| `assign_base_greedy` | `pub` | CLI / layout 默认入口保持不变 |
| `assign_shift` | `pub` | 单班主入口保持不变 |
| `assign_shift_with_plan` | `pub` | 轮换层读取 `peak_plan` 依赖 |
| `assign_shift_with_plan_skip` | crate 内部优先 | 若仍需测试或轮换调用，维持最小可见性 |
| `AssignBaseOptions` / `AssignShiftResult` | `pub` | 保持 serde / 调用方兼容 |
| `assignment_operator_names` / `rotating_workers` / `pinned_assignment` | `pub` 或按调用点收缩 | 拆分前先查 `schedule/`、CLI 调用点 |
| `assign_team_producer_rooms` / `assign_team_gamma_half` | `pub` 或 `pub(crate)` | 当前被 `schedule/team_rotation.rs` 使用，迁入 `team_fill.rs` 后 re-export |
| `assign_power_stations` / `assign_power_rooms` | `pub` 或 `pub(crate)` | 若仅内部使用，迁移后收缩 |
| `assign_control` | `pub(crate)` | 中枢补位内部入口 |

未列出的 helper 默认私有；确需跨子模块共享时用 `pub(super)` 或 `pub(crate)`，不对 crate 外暴露。

## 职责边界

| 职责 | 目标归属 | 说明 |
|------|----------|------|
| 公开单班 API | `layout::assign::mod` | 保持调用方稳定 |
| 单班执行顺序 | `assign/pipeline.rs` | 只描述阶段顺序和 resolve 时机 |
| `used`、layout 快照、durin 计数 | `assign/run.rs` | 避免参数列表继续膨胀 |
| 数据驱动 system 认领 | `layout/orchestrate/` | 不调贸易/制造/发电 search |
| 代码化体系认领（降级/替代/gate） | `layout/system_integrity/`（→`systems/`） | 产出 anchor/producer/constraint/degradation 进 `build_plan`，不调 search、不算效率 |
| 中枢补位 | `assign/control_fill.rs` | 允许调用 `search_control_combos` |
| producer 落位 | `assign/producer_fill.rs` | 统一为 ProducerSlot 消费，不再逐体系手写 |
| 贸易余站 + anchor 补齐 | `assign/trade_fill.rs` | 允许调用 `search/role_pick.rs` 与贸易搜索 |
| 制造余站 + anchor 补齐 | `assign/manufacture_fill.rs` | 允许调用制造搜索；公孙金线先搬迁后语义化 |
| 发电余站 | `assign/power_fill.rs` | 允许调用发电搜索 |
| 轮换半区填房 | `assign/team_fill.rs` | 被 `schedule/team_rotation.rs` 调用 |
| 提交房间与效率快照 | `assign/commit.rs` | 贸易/制造/发电共用 |
| 全局资源效率结算 | `resolve_base` / `cross_facility` | 不变；体系层只决定 producer 是否上岗 |

## Pipeline 阶段门

拆分不能改变当前单班落位的语义顺序。`pipeline.rs` 至少保留以下阶段门，并补“两路径汇合”与“阶段性候选池”：

```text
seed / pinned assignment
  -> build_plan
       数据驱动 registry + 代码化体系层（evaluate_systems）汇合
       产出 fixed / anchor(reserved) / producer / constraint / degradation
  -> execute_plan
       reserved：落核心，不补满；只落硬 producer / fixed core
  -> control fill
  -> producer placement（统一 ProducerSlot）
  -> producer resolve_snapshot
  -> power fill
  -> anchor trade fill   （required：阶段性候选池 + must_include + forbid_with）
  -> anchor manufacture fill
  -> plain trade / plain manufacture fill
  -> final assignment / resolve
```

凡影响 `LayoutContext.global` / `global_inject` 的 producer 必须先落位并经过 `resolve_snapshot`，再搜索依赖全局资源的 consumer 房。拆分时不要把 `resolve_base` 逻辑搬进 assign；assign 只决定何时取快照、把快照传给后续搜索。

## anchor 三态（数据流摘要）

| 状态 | 谁产生 | 对 `assignment` | 对 `used` | 房间满员 |
|------|--------|-----------------|-----------|----------|
| reserved | execute（消费 plan anchor） | 只放核心 | 核心计入 | 否（可未满 3） |
| required | anchor fill（trade/manufacture） | 不变（搜索中） | 不变 | 搜索补齐中 |
| committed | commit helper | 写满 3 人 | 全员计入 | 是 |

阶段性候选池：`required` 阶段的 pool 必须按“已启动体系 + 当前 box”构造，保证 anchor 在池中。详见 `CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md` §5。

## AssignmentRun 不变式

Phase 2 可引入内部运行上下文收敛参数：

```rust
pub(crate) struct AssignmentRun<'a> {
    blueprint: &'a BaseBlueprint,
    instances: &'a OperatorInstances,
    table: &'a SkillTable,
    options: &'a AssignBaseOptions,
    durin_plan: Option<u32>,
    assignment: BaseAssignment,
    used: HashSet<String>,
}
```

`AssignmentRun` 只用于 assign 内部，提供 `resolve_snapshot(...)`、`build_pools(...)`、`mark_used(...)`、`is_room_empty(...)` 等 helper。

必须保持这些规则：

- 不把 `AssignmentRun` 暴露到 `layout/orchestrate`、`system_integrity`、`search` 或 CLI。
- 不把机制公式塞进 `AssignmentRun`。
- `used` 只能通过 commit helper 更新；禁止填房函数直接改 `assignment` 后忘记同步 `used`。
- commit 后 debug assert：`used` 与 `assignment` 中已占岗位人员一致（anchor reserved 阶段的“核心计入、队友未计入”是显式例外，由三态规则定义，不算漂移）。
- seeded / pinned 房间、`training_assist`、`base_workforce` 是否计入 `used` 必须在 `run.rs` 中集中定义。
- 先替换参数最长、重复最多的路径：trade / manufacture / power / team。

## 迁移顺序

把原“迁移约束”里互相 defer 的语义治理项，改写为与 facade 拆分协调排序的 Phase。**先拆基底、再上语义层**，每轮行为可解释：

1. **基底（facade 拆分）**：机械拆分 `assign/` 子模块，行为等价。不在本轮改策略语义。
2. **语义骨架**：`orchestrate/plan.rs` 增 anchor / producer / constraint / degradation 类型（仅类型 + 序列化，不改排班结果）。
3. **体系层接线**：`evaluate_systems` 接入 `build_plan`，迷迭香走代码化路径产出 plan 片段；`rosemary_perception` 降级/移除；行为对照 full-E2 243 fixture。
4. **execute 三态 + anchor fill 阶段性候选池**：execute 按状态分支，reserved 不补满；anchor fill 用阶段性候选池消除 must-include 报错。
5. **制造对称接口**：`manufacture_fill` 补 `must_include` / `min_pick` / `forbid_with` / optional line；迁红松林、自动化金线。
6. **producer 统一**：`producer_fill.rs` 手写 producer 收敛为 ProducerSlot；深巡乌尔比安宿舍 anchor 纳入。
7. **轮换接入**：`team_rotation` 消费 plan 的 anchor / producer / degradation / shift_bind，不再从房间名反推（plan 语义稳定后做）。

原 0001 的“不要在同一轮同时处理 `skip_trade_core_registry_systems` 删除 / `pick_trade_meta_then_plain` role 迁移 / 公孙金线语义化 / 感知 producer 迁移”不再是非完成条件，而是上列 Phase 3–6 分轮承载。机械拆分（Phase 1）期间仍不改变这些策略语义。

## 非目标

- 不把单班排班改成全局最优、整数规划、模拟退火或 `C(n,3)^站数`。
- 不在本轮拆分中改变贸易/制造/发电搜索评分。
- 不把 `resolve_base` 逻辑搬进 assign；不在体系层重算效率公式。
- 不为了拆文件同步清理所有 global hardcode；语义迁移按 Phase 分轮。
- 不一次性重写 `team_rotation`（等 plan 语义稳定后接入）。
- 不强制把所有体系下沉 JSON，也不删除 `system_integrity`。
- 不改变 CLI 输出契约和现有 `plan` / `layout test` 行为。

## 验收口径

最低验收是**行为等价或可解释漂移**，而不只是能编译。

```powershell
New-Item -ItemType Directory -Force target/codex-logs | Out-Null

cargo test -p infra-core --no-run *> target/codex-logs/infra-core-test-build.log
Get-Content target/codex-logs/infra-core-test-build.log -Tail 80
cargo test -p infra-core --quiet

cargo build -p infra-cli *> target/codex-logs/infra-cli-build.log
Get-Content target/codex-logs/infra-cli-build.log -Tail 80
cargo run -q -p infra-cli -- verify --all

cargo run -q -p infra-cli -- plan `
  --operbox data/fixtures/243/operbox_full_e2.json `
  --maa-out out/243_maa.json
```

- 机械拆分（Phase 1）前后保存并对比默认 `plan` 输出或 MAA JSON 的 normalized diff。允许字段顺序、日志顺序变化；不允许房间编制、shortcut 命中、效率快照出现**无解释**漂移。
- 语义层 Phase 的漂移必须能用体系决策解释（如 anchor 改变了某贸易站第三人），并由 `CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md` §7 回归矩阵守住。

测试迁移原则：

- facade 保留 public API / 端到端测试。
- 制造候选池测试迁到 `manufacture_fill.rs`；commit / snapshot 测试迁到 `commit.rs`；power / trade / team 专属测试跟随对应模块。
- 体系层（迷迭香 evaluator / 降级阶梯）测试随 `system_integrity` 接线一并转为生产路径断言。

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 机械拆分时误改行为 | 每次只搬一类函数；用默认模拟输出 diff 守住行为等价 |
| 私有 helper 可见性膨胀 | 先查调用点；默认私有，必要时 `pub(super)`，最后才 `pub(crate)` |
| `used` 与 assignment 漂移 | 所有落位统一走 commit helper；`AssignmentRun` 加 debug assert；anchor reserved 例外由三态规则显式定义 |
| resolve 时机漂移 | `pipeline.rs` 明确 producer resolve 阶段门 |
| 两条体系入口重复/冲突认领 | 统一在 `AssignmentPlan` 汇合并做互斥/优先级仲裁；体系层先认领，registry 在已占资源上 skip（复用 `skip_system_ids`） |
| 代码化体系层重新变特例堆 | 每个体系实现统一 `SystemEvaluator` 接口，产出 anchor/producer/constraint/degradation，不允许直接改 `assignment` |
| 循环依赖 | `commit.rs` 只依赖 assignment / pool hit 类型，不反调 fill 模块；体系层不依赖 assign |
| 文档与实现再次漂移 | ADR 只记录边界；接口与执行清单放 `docs/TODO/` |

## 后续记录

若未来决定把 `assign/` 或体系层抽成独立 crate、引入 declarative degradation DSL，或把 registry / global policy 语义治理一次性合并实现，需要新 ADR。当前决策覆盖 `infra-core::layout` 内部模块边界、代码化体系层的存在与两路径汇合契约。
