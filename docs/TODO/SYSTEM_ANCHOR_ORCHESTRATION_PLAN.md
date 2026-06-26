# 体系 Anchor 编排计划

> 状态：ready  
> 来源：[ADR 0001](../ADR/0001-layout-assignment-decomposition.md)、[ORCHESTRATION_LAYER.md](../ORCHESTRATION_LAYER.md)、[BASE_ASSIGNMENT.md](../BASE_ASSIGNMENT.md)、[SYSTEM_REGISTRY_NORMALIZATION_REPORT.md](SYSTEM_REGISTRY_NORMALIZATION_REPORT.md)、`docs/公孙长乐的体系分析文档/`

## 目标

把“体系队先落位”和“效率计算”彻底分层：

- 编排层只按硬条件、优先级、互斥、产物约束认领体系 anchor，不调用贸易 / 制造 / 发电求解器算效率。
- L3 shortcut / role policy / 工具人搜索在 anchor 房间补齐阶段最先执行。
- 效率结算只在人员落位后由现有 L1/L2/L3、`resolve_base`、`search/*` 完成。
- 公孙体系文档作为 topology truth：核心、可选 producer、consumer、互斥、降级路径优先信文档；具体数值用当前数据和回归校准。

明确不做：

- 不引入全局联合最优、整数规划、模拟退火或 `C(n,3)^站数`。
- 不把迷迭香、红松林、自动化写成 `assign.rs` / `manufacture_fill.rs` 的临时特判。
- 不在编排层重复实现效率公式。
- 不在本计划内重写 L1/L2/L3 机制。

## 背景判断

当前体系失效的症状不是单个公式错误，而是编排语义不够细：

- 有些体系被写成 fixed 三人组，导致缺外围工具人时整条关闭。
- 有些 producer 已能进入全局资源池，但 consumer 没有 anchor，后续贪心可能不保留核心。
- 黑键可通过 `closure` 贸易 role 上班，但迷迭香没有同等级的制造 consumer anchor，于是会出现“只看到黑键上班”。
- 红松林目前把中枢、经验线、赤金砾线绑成 all-or-nothing，且部分 slot 仍依赖固定 `room_id`。

正确边界应是：`layout/orchestrate` 认领体系结构，`assign/*_fill.rs` 补齐 anchor 房，求解器只消费最终编制。

## 目标流水线

```text
assign_shift()
  -> build_plan
       只选体系，不算效率
       输出 fixed_slots + anchor_slots + optional_producers + constraints
  -> execute_plan
       只落硬 producer / fixed core / anchor 占坑
       可留下未满 3 人的贸易或制造房
  -> producer resolve_snapshot
       全局资源 producer 生效
  -> fill anchored trade rooms
       固定核心后走 role / L3 shortcut / search 补齐
  -> fill anchored manufacture rooms
       固定核心后走制造搜索补齐
  -> fill power / plain trade / plain manufacture
       剩余房间按现有贪心
  -> resolve_base / score
       统一结算效率
```

## 数据语义

### System Slot 分类

现有 `fixed` / `bond` / `pick_one` 不足以表达体系队。需要补以下语义，优先兼容旧 schema：

| 概念 | 含义 | 示例 |
|------|------|------|
| `fixed_slot` | 已完整确定的房间，直接落位 | 但书完整叙拉古站、少数严格三人组 |
| `anchor_slot` | 先占房并固定核心，剩余位置后续搜索 | 迷迭香制造、黑键贸易、清流+温蒂赤金线 |
| `producer_slot` | 只负责全局 / 跨设施资源，不直接表示产出房 | 絮雨办公室、爱丽丝宿舍、焰尾中枢 |
| `optional_slot` | 缺失时裁剪，不拖死核心体系 | 感知外围 producer、红松砾赤金线 |
| `min_pick_slot` | 从候选中至少选 N 人，剩余交给搜索 | 灰毫 / 远牙 / 野鬃至少 2 人 |
| `constraint` | 不占房但影响补齐搜索 | 黑键不与巫恋同站、迷迭香不与清流+温蒂同房 |

### AssignmentPlan 扩展

建议在 `layout/orchestrate/plan.rs` 增加内部结构：

```rust
pub struct AnchorSlot {
    pub system_id: String,
    pub room_id: RoomId,
    pub facility: FacilityKind,
    pub required: Vec<AssignedOperator>,
    pub fill_policy: AnchorFillPolicy,
    pub constraints: Vec<AnchorConstraint>,
}
```

`AssignmentPlan` 保留现有 `registry_claims` 兼容字段，同时新增：

- `anchor_slots`
- `optional_producer_slots`
- `system_diagnostics` 或后续 explain report 引用

## 分阶段实施

### Phase 0：System Explain（已完成）

先加可观测性，不改变排班结果。

落地状态：

- `layout/system.rs` 提供 `explain_registry_systems` 与结构化 `SystemExplainReport`。
- `assign.rs` 提供 `explain_assignment_systems`，复用主路径 trade role registry skip 规则。
- CLI 增加 `layout test --explain-systems`；配合 `--json` 时只输出 explain JSON。

动作：

- [x] 在 `layout/system.rs` 增加 `explain_registry_systems`。
- [x] 输出每条 system 的 selected / skipped 状态和原因。
- [x] 原因至少包含：缺干员、精英化不足、干员已占用、房间不可用、产物不匹配、中枢容量不足、互斥组已被占、班次模式不允许。
- [x] CLI 增加调试入口：`layout test --explain-systems` / `layout test --explain-systems --json`。

验收：

- [x] 不改变 `plan` / `layout team-rotation` 的编制结果。
- [x] 能解释 `pinus_sylvestris`、`automation_group`、`docus_syracusa`、`blackkey_closure` 等 selected / skipped 原因。

### Phase 1：Anchor 数据模型

动作：

- 扩展 `base_systems.json` schema，支持 `anchor` / `fill: "search"` / `min_pick` / `constraints`。
- `layout/system.rs` 解析新字段，旧 `operators` fixed 语义保持兼容。
- `build_plan` 产出 anchor，不调用 solve。
- `execute_plan` 能把 anchor 核心落入房间并标记 `used`，允许房间未满员。

优先迁移对象：

| 体系 | 目标表达 |
|------|----------|
| 迷迭香感知 | 迷迭香制造 anchor + 黑键贸易 anchor + 感知 producer optional |
| 自动化组 | 清流+温蒂赤金制造 anchor + 发电 producer |
| 红松林 | 焰尾+薇薇安娜 producer + 红松经验 `min_pick` anchor，砾赤金 optional |

验收：

- 编排层仍不调用贸易 / 制造 solve。
- `AssignmentPlan` 能显示体系 anchor 已认领，但房间可以等待 fill 阶段补齐。

### Phase 2：Anchor 房补齐

动作：

- `assign/trade_fill.rs` 先补齐已有贸易 anchor 房，再填普通贸易房。
- `assign/manufacture_fill.rs` 先补齐已有制造 anchor 房，再填普通制造房。
- 搜索接口支持 `must_include` / `forbid_with` / `reserved_room` 这类过滤。
- 贸易 anchor 补齐时仍优先通过 role policy / L3 shortcut；编排层不计算 shortcut 分数。

规则：

| Anchor | 补齐策略 |
|--------|----------|
| 黑键贸易 | 必须包含黑键；禁止巫恋同房；优先可露 / 但书兼容 role；L3 shortcut 命中则短路 |
| 迷迭香制造 | 必须包含迷迭香；禁止清流+温蒂同房；队友由制造搜索补齐 |
| 清流+温蒂赤金 | 必须在赤金线；第三人搜索，森蚺 / 冬时只是候选优先，不写死 |
| 红松经验 | 至少 2 名红松制造干员；第三人搜索 |

验收：

- full E2 243 仍能得到但书、黑键、迷迭香、自动化、红松林的合理落位。
- 缺外围 producer 时核心 anchor 不被普通贪心打散。
- 缺核心时体系关闭，并在 explain 中明确原因。

### Phase 3：红松林 / 感知 / 自动化迁移

动作：

- 以 `docs/公孙长乐的体系分析文档/` 的结构描述为准，迁移三条体系。
- 去掉对固定 `room_id` 的不必要依赖，优先使用 `recipe`。
- 红松林拆为核心经验线和可选砾赤金线，避免可选赤金线拖死经验核心。
- 感知 producer 继续由 global resource / `resolve_base` 结算，但 consumer anchor 由 plan 保住。

验收：

- `pinus_sylvestris` 不再因为砾赤金 optional 缺失导致经验线核心关闭。
- 迷迭香+黑键满足核心时同班出现，并继续由 `shift_bind` 保证同上同下。
- 自动化金线不与迷迭香制造 anchor 同房。

### Phase 4：AssignmentRun 与 ADR 0001 拆分收束

动作：

- 按 ADR 0001 引入 `AssignmentRun` / commit helper，收敛 `used` 与 `assignment` 修改入口。
- 将 anchor fill 分别归入 `trade_fill.rs`、`manufacture_fill.rs`、`team_fill.rs`。
- `pipeline.rs` 明确 anchor fill 阶段门。

验收：

- `assign.rs` 保持 public facade，不再新增体系特例。
- commit 后 debug assert：`used` 与 `assignment.operator_names()` 一致。
- 轮换 γ 半区复用同一套 anchor fill，不再另写半区特例。

## 改动范围

| 文件/目录 | 动作 |
|-----------|------|
| `crates/infra-core/src/layout/system.rs` | 增加 explain；解析 anchor / constraint 字段 |
| `crates/infra-core/src/layout/orchestrate/plan.rs` | 增加 anchor slot / constraint 类型 |
| `crates/infra-core/src/layout/orchestrate/select.rs` | 选型时产出 anchor，不算效率 |
| `crates/infra-core/src/layout/orchestrate/execute.rs` | 落位 fixed / anchor core，允许未满房 |
| `crates/infra-core/src/layout/assign/pipeline.rs` | 增加 anchor fill 阶段门 |
| `crates/infra-core/src/layout/assign/trade_fill.rs` | 先补贸易 anchor，再补普通贸易 |
| `crates/infra-core/src/layout/assign/manufacture_fill.rs` | 先补制造 anchor，再补普通制造 |
| `crates/infra-core/src/layout/assign/team_fill.rs` | 轮换半区复用 anchor fill |
| `data/base_systems.json` | 迁移迷迭香、自动化、红松林为 anchor / optional / min_pick |
| `docs/ORCHESTRATION_LAYER.md` | 更新 System schema 与流水线事实 |
| `docs/BASE_ASSIGNMENT.md` | 更新落位顺序与职责边界 |
| `docs/SCHEDULE_ROTATION.md` | 更新 shift_bind 与 anchor 在轮换中的关系 |

## 回归矩阵

- [ ] `cargo test -p infra-core --quiet`
- [ ] `cargo run -q -p infra-cli -- verify --all`
- [ ] `cargo run -q -p infra-cli -- plan --operbox data/fixtures/243/operbox_full_e2.json --maa-out out/243_maa.json`
- [ ] full E2：迷迭香、黑键、自动化金线、红松经验线均能落位。
- [ ] 缺迷迭香或黑键：感知链关闭，普通贪心接管。
- [ ] 缺絮雨但有核心：explain 显示降级 producer 缺失，不误报核心缺失。
- [ ] 缺一名红松制造：红松经验线仍可用搜索补第三人。
- [ ] 无薇薇安娜：红松林核心关闭。
- [ ] 黑键不会与巫恋同贸易站。
- [ ] 清流+温蒂不会与迷迭香同制造站。
- [ ] 轮换中迷迭香+黑键同上同下、上 2 休 1。

## 完成后归档

完成后移动到 `docs/ARCHIVE/done/`，并更新：

- [ORCHESTRATION_LAYER.md](../ORCHESTRATION_LAYER.md)
- [BASE_ASSIGNMENT.md](../BASE_ASSIGNMENT.md)
- [SCHEDULE_ROTATION.md](../SCHEDULE_ROTATION.md)
- [SYSTEM_CHAINS.md](../SYSTEM_CHAINS.md)
