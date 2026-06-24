# 排班轮换（Schedule Rotation）

> **现行**：αβγ **ABC 三队轮换**（`layout team-rotation` / `schedule_team_rotation`）。  
> **已废弃**：全基建 **A-B-A**（`layout rotation` / `schedule_base_rotation_a_b_a`）——仅保留兼容，新功能不再维护。

---

## 1. 两种模型对比

| | **ABC αβγ**（现行） | **A-B-A**（废弃） |
|---|---------------------|-------------------|
| CLI | `layout team-rotation` | `layout rotation`（启动时打印废弃警告） |
| 核心 API | `schedule_team_rotation` | `schedule_base_rotation_a_b_a` |
| 班次结构 | 12h + 6h + 6h；每班 **两队上岗、一队休息** | 高峰 → 恢复 → **复用高峰** |
| 生产设施 | 切成 H1/H2 两半；α/β 来自 peak 切半，γ 替补 | 每班整图重搜高峰/恢复 |
| 中枢 / 宿舍 | 宿舍/办公室三班钉死；中枢按 αβγ 轮休重分配，每班补满 5 人 | 中枢/宿舍三班钉死 |
| 设施空转 | **禁止**（每班满编） | 允许恢复班降配 |
| 默认模拟 | ✅ [AGENTS.md](../AGENTS.md) §6.2 | ❌ 不要用 |

用户说「跑一遍模拟」「三班模拟」时，一律用 **`layout team-rotation`** + `--maa-out`，见 [INFRA_CLI.md](INFRA_CLI.md)。

---

## 2. ABC 轮换流程

```
peak = assign_shift_with_plan(Peak) → { assignment, plan }
shared = pinned_assignment(peak)     # 宿舍/办公室三班钉死；中枢不钉死
[h1, h2] = split_production_facilities
align_shift_binds(h1, h2)            # 迷迭香+黑键等同队
α = peak ∩ h1,  β = peak ∩ h2
γ = assign_team_gamma_half(h1) + assign_team_gamma_half(h2)  # plain 贸易，不重搜 meta
team_ctrl = build_team_control_map(peak.control, plan, h1)    # 中枢干员归入 αβγ
team_ctrl += core inject / mood-cost control candidates       # 效率注入/心情类散件

S1 (12h): shared + control(α+β) + α(H1) + β(H2)   休息 γ
S2 (6h):  shared + control(β+γ) + β(H2) + γ(H1)   休息 α
S3 (6h):  shared + control(γ+α) + γ(H2) + α(H1)   休息 β
```

γ 替补贸易与 peak `assign_trade_remainder` 同路径（`trade_hit_ok_for_greedy`），制造/发电仍站绑定贪心。

实现：`crates/infra-core/src/schedule/team_rotation.rs`。

---

## 3. 中枢轮休规则

中枢现行规则是**按队伍轮休**，不是三班钉死同一套 5 人。目标：

- 每班中枢满编 5 人。
- 休息队的中枢干员不进入当班中枢。
- 每个中枢干员在 αβγ 周期内至少休息 1 班。
- 优先保留贸易 / 制造效率注入；资源-only producer 不作为无消费方补位。

中枢干员归队：

1. 体系绑定中枢位跟随其生产体系所在半区：H1 → α，H2 → β；纯中枢体系默认 α。
2. peak 中枢里的非体系散件按当前队伍人数最少优先均分到 α/β/γ。
3. 额外中枢候选只收两类：核心贸易/制造注入，或心情消耗类补位。
4. 若额外候选已经在生产队中，跟随该生产队；否则分到当前中枢人数最少的队。

每个班次只用活跃两队的中枢候选建池。体系中枢位先 pin 到 `control`，再由 `assign_control` 补满 5 人。轮换内设置 `skip_standalone_control = true`，中枢搜索按 `Efficiency` 策略排序，即 `ControlInjectRawSumV0`：`trade_inject + manu_gold + manu_br`。这是中枢候选的局部排序 policy，不是贸易/制造平衡公式。

S2 有深海短班特例：若可构造歌蕾蒂娅中枢 + 深海制造候选，且制造评分优于普通 S2，才采用该路径；深海链不进入 12h 主班。

相关单测：

- `team_rotation_control_center_rest_rotates`
- `team_rotation_control_center_respects_resting_team`
- `team_rotation_control_prefers_trade_manu_inject_over_resource_only_fillers`
- `team_rotation_abyssal_only_runs_in_s2_short_shift`

---

## 4. 243 长班贸易取舍

243 双贸 full E2 / 公孙高配上下文中，peak 编制会把两个 18h 长班贸易站固定为：

| 队 | 贸易站 | 班次 |
|----|--------|------|
| α | 但书 + 伺夜 + 贝洛内 | S1 + S3，共 18h |
| β | 可露希尔 + 黑键 + 吉星 | S1 + S2，共 18h |

这是一条**上下文 meta 冲突策略**，不是全局 priority 改动：`blackkey_closure` 在 `base_systems.json` 的普通优先级仍低于 `witch_long_beta`。只有当 `docus_syracusa` 已选中、迷迭香 / 黑键 / 可露希尔 / 吉星均 E2，且有 E2 感知源（絮雨 / 八幡海铃 / 焰狐龙梓兰）时，`select_registry_systems` 才临时让 `blackkey_closure` 覆盖 `witch_long_beta`，以保证黑键与迷迭香同上同下并把可露希尔站放进 12h 长班。

普通场景仍保持 `witch_long_beta` 优先于 `blackkey_closure`。单测：

- `claim_docus_long_shift_prefers_blackkey_closure_over_witch`
- `claim_witch_long_beta_still_beats_blackkey_closure_without_docus`
- `team_rotation_docus_and_blackkey_closure_share_12h_shift`

---

## 5. 班次绑定（shift_bind）

部分干员须 **同上同下、上 N 休 M**，在 schedule 层处理（非编排层、非 global effect）。

| 绑定 ID | 干员 | 规则 | 模块 |
|---------|------|------|------|
| `rosemary_blackkey` | 迷迭香、黑键 | 同队；αβγ 周期内上岗 2 班、休息 1 班 | `schedule/shift_bind.rs` |

**对齐**：若 peak 编制下绑定组成员落在不同 H1/H2 半区，`align_shift_binds_in_halves` 交换同类设施房间，使二者进入同一 cohort（α 或 β）。

**休息班次**（与队伍标签绑定）：

| 队 | 休息班 |
|----|--------|
| γ | S1（12h） |
| α | S2（6h） |
| β | S3（6h） |

单测：`team_rotation_rosemary_blackkey_shift_bind`。

---

## 6. CLI 与 MAA

```bash
cargo run -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

- stderr：人类可读三班排班表 + 队伍花名册  
- `--maa-out`：MAA 排班 JSON（见 `export/maa.rs`）

`layout rotation` 仍会运行，但 **stderr 会提示废弃**；MAA 描述字段可能仍含 legacy「ABA」字样，不影响 ABC 路径。

---

## 7. 与编排层的关系

- **单班编制**（peak/recovery）：`assign_shift` → 编排 `System → Plan → Execute`（见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)）。
- **多班轮换**：在 peak 编制之上切半 + γ 替补，并在 schedule 层做中枢 αβγ 归队；**不**在编排层做 shift_bind。
- 宿舍/办公室：由 `pinned_assignment(peak)` 从 peak 拷贝，三班钉死。
- 中枢：不在 `pinned_assignment` 中钉死；每班按活跃两队重搜/补位，休息队不得上岗。
- 迷迭香感知 producer：不作为完整 `rosemary_perception` System 进编；由 global effect + `assign_perception_producers` 处理（Phase 4）。
- 黑键贸易站：`blackkey_closure` 已作为低优先级 same-station registry 组合进编；在但书长班上下文中覆盖龙巫，其余场景仍低于龙巫。

`TeamRotationReport.peak_plan` 携带完整 `AssignmentPlan`（JSON 可序列化）；text 输出打印已选体系与贸易 meta 房间。

---

## 8. 相关文件

| 文件 | 作用 |
|------|------|
| `schedule/team_rotation.rs` | ABC 主流程 |
| `schedule/shift_bind.rs` | 班次绑定定义与对齐 |
| `layout/assign.rs` | `pinned_assignment`、`assign_control`、`assign_team_gamma_half`（γ plain 贸易） |
| `search/control.rs` | 中枢候选搜索、`ControlInjectRawSumV0` 排序、补位策略 |
| `schedule/base_rotation.rs` | A-B-A legacy + `score_base_assignment`（ABC 复用评分） |
| `infra-cli/commands/layout.rs` | `team-rotation` / `rotation` 子命令 |
| `export/maa.rs` | MAA JSON 导出 |

---

## 9. Agent 提示

- **跑模拟** → `layout team-rotation`，不要用 `layout rotation` 或 `layout test`。
- **改中枢轮休 / 补位** → `schedule/team_rotation.rs` + `layout/assign.rs` + `search/control.rs`；不要把中枢重新放回 `pinned_assignment`。
- **改迷迭香/黑键同休** → `shift_bind.rs` + `team_rotation.rs`，不要改 `base_rotation.rs` 的 A-B-A 逻辑。
- **改但书 / 龙巫 / 可露希尔 meta 取舍** → `base_systems.json` + `layout/system.rs` 的命名上下文策略；不要单纯调高 `blackkey_closure` 全局 priority。
- **改 peak 编制** → 编排层 / `assign_shift`，见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)。
