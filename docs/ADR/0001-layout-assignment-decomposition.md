# ADR 0001: layout assignment 编排拆分

> 决策状态：accepted
> 实现状态：pending
> 日期：2026-06-26
> 关联文档：[../ORCHESTRATION_LAYER.md](../ORCHESTRATION_LAYER.md)、[../BASE_ASSIGNMENT.md](../BASE_ASSIGNMENT.md)、[../TODO/SYSTEM_REGISTRY_NORMALIZATION_REPORT.md](../TODO/SYSTEM_REGISTRY_NORMALIZATION_REPORT.md)

## 背景

`crates/infra-core/src/layout/assign.rs` 当前是全基建单班与轮换填房的事实入口。它已经调用 `layout/orchestrate::{build_plan, execute_plan}`，但主流程和大量策略仍集中在一个文件中：

- `assign_shift_with_plan_skip` 串联 seed、registry plan、producer、resolve、建池、发电、贸易、制造。
- 中枢补位、宿舍/办公室 producer、感知 producer、深巡/乌尔比安宿舍锚点都在同一层。
- 贸易核心优先通过 `pick_trade_meta_then_plain` 调 `search/role_pick.rs`，但还保留 `skip_trade_core_registry_systems` 跳过旧 registry 抢站条目。
- 制造存在公孙金线固定锚点、候选池扩展、容量兜底等局部策略。
- `assign_team_producer_rooms`、`assign_team_gamma_half` 等轮换半区填充函数也放在同一文件。

这不是机制层错乱。L1/L2/L3 求解、`resolve_base`、`search/*` 的职责基本清楚。问题是 **assignment orchestration facade 过胖**：宏观流水线、设施填房 policy、producer policy、轮换填充 helper、提交/快照工具混在一起，导致后续新增体系时容易继续把特例塞回 `assign.rs`。

本 ADR 的 `accepted` 表示模块边界决策已接受，不表示拆分已经完成。具体实施清单应放入 `docs/TODO/`；本文只记录为什么这样拆、拆完后边界如何保持。

## 决策

保留现有公开入口，不做大重写，不引入全局联合最优。将 `layout/assign.rs` 收敛为一个薄 facade，并把内部实现拆为若干按职责命名的子模块。

目标模块结构：

```text
crates/infra-core/src/layout/assign/
  mod.rs              # public facade: assign_base_greedy / assign_shift / result/options
  pipeline.rs         # 单班流水线：阶段顺序、resolve 阶段门、fillers 调度
  run.rs              # AssignmentRun / FillContext：blueprint、options、used、durin、layout 快照
  commit.rs           # commit room、names_disjoint、efficiency snapshot
  control_fill.rs     # 中枢补位
  producer_fill.rs    # 宿舍/办公室/global producer 的临时落位入口
  trade_fill.rs       # 贸易余站、role priority、恢复班孑站
  manufacture_fill.rs # 制造产线、候选池、容量兜底、当前公孙金线锚点
  power_fill.rs       # 发电站填充
  team_fill.rs        # αβγ 半区填充 helper
```

`run.rs` 避免与既有 `layout/context.rs` 撞名。`layout/orchestrate/` 保持为 registry plan 层，不吸收设施搜索逻辑：

```text
layout/orchestrate/
  plan/select/execute # 只处理 System 选型与 fixed/bond/pick_one 落位
```

## API 保留

拆分期间保持调用方稳定。当前可见入口按下表处理：

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

未列出的 helper 默认私有；确需跨子模块共享时使用 `pub(super)` 或 `pub(crate)`，不对 crate 外暴露。

## 职责边界

| 职责 | 目标归属 | 说明 |
|------|----------|------|
| 公开单班 API | `layout::assign::mod` | 保持调用方稳定 |
| 单班执行顺序 | `assign/pipeline.rs` | 只描述阶段顺序和 resolve 时机 |
| `used`、layout 快照、durin 计数 | `assign/run.rs` | 避免参数列表继续膨胀 |
| registry system 认领 | `layout/orchestrate/` | 不调贸易/制造/发电 search |
| 中枢补位 | `assign/control_fill.rs` | 允许调用 `search_control_combos` |
| producer 临时落位 | `assign/producer_fill.rs` | 感知/宿舍 producer 先集中，后续迁 global policy |
| 贸易余站 | `assign/trade_fill.rs` | 允许调用 `search/role_pick.rs` 与贸易搜索 |
| 制造余站 | `assign/manufacture_fill.rs` | 允许调用制造搜索；公孙金线先搬迁后语义化 |
| 发电余站 | `assign/power_fill.rs` | 允许调用发电搜索 |
| 轮换半区填房 | `assign/team_fill.rs` | 被 `schedule/team_rotation.rs` 调用 |
| 提交房间与效率快照 | `assign/commit.rs` | 贸易/制造/发电共用 |

## Pipeline 阶段门

拆分不能改变当前单班落位的语义顺序。`pipeline.rs` 至少保留以下阶段门：

```text
seed / pinned assignment
  -> build_plan
  -> execute_plan
  -> control fill
  -> dorm / office / perception / global producer fill
  -> producer resolve_snapshot
  -> power fill
  -> trade remainder fill
  -> manufacture line fill
  -> final assignment
```

凡影响 `LayoutContext.global` / `global_inject` 的 producer 必须先落位并经过 `resolve_snapshot`，再搜索依赖全局资源的 consumer 房。拆分时不要把 `resolve_base` 逻辑搬进 assign；assign 只决定何时取快照、把快照传给后续搜索。

## AssignmentRun 不变式

Phase 2 可引入内部运行上下文，用于收敛参数：

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

- 不把 `AssignmentRun` 暴露到 `layout/orchestrate`、`search` 或 CLI。
- 不把机制公式塞进 `AssignmentRun`。
- `used` 只能通过 commit helper 更新；禁止填房函数直接改 `assignment` 后忘记同步 `used`。
- commit 后 debug assert：`used` 与 `assignment` 中已占岗位人员一致。
- seeded / pinned 房间、`training_assist`、`base_workforce` 是否计入 `used` 必须在 `run.rs` 中集中定义。
- 先替换参数最长、重复最多的路径：trade / manufacture / power / team。

## 迁移约束

实现时建议分两步降低风险：

1. 先保留 `layout/assign.rs` 作为 facade，在 `layout/assign/` 下逐步新增子模块并移动函数。
2. 行为等价后，再把 facade 改为 `layout/assign/mod.rs`。

机械拆分期间不改变策略语义。尤其不要在同一轮改动中同时处理：

- `skip_trade_core_registry_systems` 的删除或收缩；
- `pick_trade_meta_then_plain` 的 role policy 迁移；
- 公孙金线固定锚点语义化；
- 感知 producer / 深巡 / 乌尔比安等 global policy 迁移。

这些属于 registry / global resource 语义治理，应由 `docs/TODO/SYSTEM_REGISTRY_NORMALIZATION_REPORT.md` 或新的 TODO 承载，不作为本 ADR 完成条件。

## 验收口径

拆分完成的最低验收是行为等价，而不只是能编译。

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

建议在机械拆分前后保存并对比默认 `plan` 输出或 MAA JSON 的 normalized diff。允许字段顺序、日志顺序变化；不允许房间编制、shortcut 命中、效率快照出现无解释漂移。

测试迁移原则：

- facade 保留 public API / 端到端测试。
- 制造候选池测试迁到 `manufacture_fill.rs`。
- commit / snapshot 测试迁到 `commit.rs`。
- power / trade / team 专属测试跟随对应模块。

## 非目标

- 不把单班排班改成全局最优、整数规划、模拟退火或 `C(n,3)^站数`。
- 不在本次拆分中改变贸易/制造/发电搜索评分。
- 不把 `resolve_base` 逻辑搬进 assign。
- 不为了拆文件同步清理所有 global hardcode；语义迁移分阶段做。
- 不改变 CLI 输出和现有 `layout test` / `plan` 行为。

## 期望结果

拆完后，阅读入口应变为：

1. 看 `layout/assign/mod.rs` 知道 public API。
2. 看 `assign/pipeline.rs` 知道单班顺序。
3. 改贸易排班只进 `assign/trade_fill.rs` 或 `search/role_pick.rs`。
4. 改制造排班只进 `assign/manufacture_fill.rs`。
5. 改 producer/global 迁移只进 `assign/producer_fill.rs`、`cross_facility/`、`resolve.rs`。
6. 改 registry 体系只进 `layout/orchestrate/` 与 `data/base_systems.json`。

`assign.rs` 不再成为所有新特例的默认落点。

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 机械拆分时误改行为 | 每次只搬一类函数；用默认模拟输出 diff 守住行为等价 |
| 私有 helper 可见性膨胀 | 先查调用点；默认私有，必要时 `pub(super)`，最后才 `pub(crate)` |
| `used` 与 assignment 漂移 | 所有落位统一走 commit helper；`AssignmentRun` 加 debug assert |
| resolve 时机漂移 | `pipeline.rs` 明确 producer resolve 阶段门 |
| 循环依赖 | `commit.rs` 只依赖 assignment / pool hit 类型，不反调 fill 模块 |
| 测试仍堆在 facade | 先保留，随后按模块迁移测试 |
| 文档与实现再次漂移 | ADR 只记录边界；执行清单和语义治理放 `docs/TODO/` |

## 后续记录

若未来决定把 `assign/` 再拆成独立 crate、引入 declarative policy registry，或把 registry / global policy 语义治理纳入同一轮实现，需要新 ADR。当前决策只覆盖 `infra-core::layout` 内部模块边界。
