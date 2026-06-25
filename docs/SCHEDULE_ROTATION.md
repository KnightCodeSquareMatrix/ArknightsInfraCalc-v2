# 排班轮换（Schedule Rotation）

> **现行**：αβγ **ABC 三队轮换**（Agent 默认经 `plan` 触发；仅排班入口为 `layout team-rotation`；核心 API 为 `schedule_team_rotation`）。
> **已废弃**：全基建 **A-B-A**（`layout rotation` / `schedule_base_rotation_a_b_a`）——仅保留兼容，新功能不再维护。

---

## 1. 两种模型对比

| | **ABC αβγ**（现行） | **A-B-A**（废弃） |
|---|---------------------|-------------------|
| CLI | `plan`（默认，含账号分析）/ `layout team-rotation`（仅排班） | `layout rotation`（启动时打印废弃警告） |
| 核心 API | `schedule_team_rotation` | `schedule_base_rotation_a_b_a` |
| 班次结构 | 12h + 6h + 6h；每班 **两队上岗、一队休息** | 高峰 → 恢复 → **复用高峰** |
| 生产设施 | 切成 H1/H2 两半；α/β 来自 peak 切半，γ 替补 | 每班整图重搜高峰/恢复 |
| 中枢 / 宿舍 | 宿舍/办公室三班钉死；中枢按 αβγ 轮休重分配，每班补满 5 人 | 中枢/宿舍三班钉死 |
| 设施空转 | **禁止**（每班满编） | 允许恢复班降配 |
| 默认模拟 | ✅ [AGENTS.md](../AGENTS.md) §6.2 | ❌ 不要用 |

用户说「跑一遍模拟」「三班模拟」时，Agent 默认用 **`plan`** + `--maa-out`（账号分析 + αβγ 排班）；只有用户明确要求“仅排班”时才用 **`layout team-rotation`**。见 [INFRA_CLI.md](INFRA_CLI.md)。

---

## 2. ABC 轮换流程

```
peak = assign_shift_with_plan(Peak) → { assignment, plan }
shared = pinned_assignment(peak)     # 宿舍/办公室三班钉死；中枢不钉死
[h1, h2] = split_production_facilities
align_shift_binds(h1, h2)            # 迷迭香+黑键等同队
α = peak ∩ h1,  β = peak ∩ h2
γ = assign_team_gamma_half(h1) + assign_team_gamma_half(h2)  # 贸易同样走 docus/closure/witch role
team_ctrl = build_team_control_map(peak.control, plan, h1)    # 中枢干员归入 αβγ
team_ctrl += core inject / hr-mood control candidates         # 效率注入/公招心情散件

S1 (12h): shared + control(α+β) + α(H1) + β(H2)   休息 γ
S2 (6h):  shared + control(β+γ) + β(H2) + γ(H1)   休息 α
S3 (6h):  shared + control(γ+α) + γ(H2) + α(H1)   休息 β
```

γ 替补贸易与 peak `assign_trade_remainder` 同路径：金单先尝试 `docus → closure → witch`，再 plain；制造/发电仍站绑定贪心。

实现：`crates/infra-core/src/schedule/team_rotation.rs`。

---

## 3. 中枢轮休规则

中枢现行规则是**按队伍轮休**，不是三班钉死同一套 5 人。目标：

- 每班中枢满编 5 人。
- 休息队的中枢干员不进入当班中枢。
- 每个中枢干员在 αβγ 周期内至少休息 1 班。
- 优先保留贸易 / 制造效率注入；未被体系认领的 producer 不作为中枢插件补位。

中枢干员归队：

1. 体系绑定中枢位跟随其生产体系所在半区：H1 → α，H2 → β；纯中枢体系默认 α。
2. peak 中枢里的非体系散件按当前队伍人数最少优先均分到 α/β/γ。
3. 额外中枢候选只收两类：核心贸易/制造注入，或公招/心情回复类补位。
   未被 `base_systems` 认领的状态 producer（热情、木天蓼、情报储备等）不作为普通插件放入中枢。
4. 若额外候选已经在生产队中，跟随该生产队；否则分到当前中枢人数最少的队。

每个班次只用活跃两队的中枢候选建池。体系中枢位先 pin 到 `control`，再由 `assign_control` 补满 5 人。轮换内设置 `skip_standalone_control = true` 以保留体系 pin，但补位仍按 plugin 规则过滤；排序分为 `ControlInjectRawSumV0` 注入分量（`trade_inject + manu_gold + manu_br`）加公招/心情补位分。这是中枢候选的局部排序 policy，不是贸易/制造平衡公式。

S2 有深海短班特例：若可构造歌蕾蒂娅中枢 + 深海制造候选，且制造评分优于普通 S2，才采用该路径；深海链不进入 12h 主班。

相关单测：

- `team_rotation_control_center_rest_rotates`
- `team_rotation_control_center_respects_resting_team`
- `team_rotation_control_prefers_trade_manu_inject_over_resource_only_fillers`
- `team_rotation_abyssal_only_runs_in_s2_short_shift`

---

## 4. 243 贸易 core role

243 双贸中，贸易站不再依赖 `blackkey_closure` / `witch_long_beta` fixed registry 早占房。peak 与 γ 替补都走同一条 role 顺序：

1. `docus`：完整叙拉古链优先；缺伺夜/贝洛内时但书仍配最高可用工具人。
2. `closure`：可露希尔优先；有黑键时可命中 `gsl_blackkey_closure`，缺黑键时仍上可露。
3. `witch`：巫恋 + 龙舌兰优先；裁缝 β / α / 空白第三人 fallback。
4. plain：散件工具人。

full E2 / 公孙高配上下文中，常见长班形态仍可能是：

| 队 | 贸易站 | 班次 |
|----|--------|------|
| α | 但书 + 伺夜 + 贝洛内 | S1 + S3，共 18h |
| β | 可露希尔 + 黑键 + 吉星 | S1 + S2，共 18h |

但这只是 role 搜索在完整账号下的自然结果，不是“可露固定黑键吉星”或“巫恋固定裁缝 β”。单测：

- `team_rotation_docus_and_blackkey_closure_share_12h_shift`
- `team_rotation_partial_trade_meta_keeps_docus_closure_and_witch`

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
cargo run -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

- stdout：账号画像摘要 + 人类可读三班排班表 + 队伍花名册
- stderr：写出路径与运行元数据
- `--maa-out`：MAA 排班 JSON（见 `export/maa.rs`）

仅排班、不需要账号画像时：

```bash
cargo run -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

`layout rotation` 仍会运行，但 **stderr 会提示废弃**；MAA 描述字段可能仍含 legacy「ABA」字样，不影响 ABC 路径。

---

## 7. 与编排层的关系

- **单班编制**（peak/recovery）：`assign_shift` → 编排 `System → Plan → Execute`（见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)）。
- **多班轮换**：在 peak 编制之上切半 + γ 替补，并在 schedule 层做中枢 αβγ 归队；**不**在编排层做 shift_bind。
- 宿舍/办公室：由 `pinned_assignment(peak)` 从 peak 拷贝，三班钉死。
- 中枢：不在 `pinned_assignment` 中钉死；每班按活跃两队重搜/补位，休息队不得上岗。
- 迷迭香感知 producer：不作为完整 `rosemary_perception` System 进编；由 global effect + `assign_perception_producers` 处理（Phase 4）。
- 黑键贸易站：`blackkey_closure` 仍是 L3 shortcut / segment 锚点；排班入口由 `closure` role 选择，不再作为 fixed registry 早占站。

`TeamRotationReport.peak_plan` 携带完整 `AssignmentPlan`（JSON 可序列化）；text 输出打印已选体系与贸易 meta 房间。

---

## 8. 相关文件

| 文件 | 作用 |
|------|------|
| `schedule/team_rotation.rs` | ABC 主流程 |
| `schedule/shift_bind.rs` | 班次绑定定义与对齐 |
| `layout/assign.rs` | `pinned_assignment`、`assign_control`、`assign_team_gamma_half`（γ 贸易 role + plain） |
| `search/role_pick.rs` | `docus` / `closure` / `witch` 贸易 role fallback 链 |
| `search/control.rs` | 中枢候选搜索、`ControlInjectRawSumV0` 排序、补位策略 |
| `schedule/base_rotation.rs` | A-B-A legacy + `score_base_assignment`（ABC 复用评分） |
| `infra-cli/commands/layout.rs` | `team-rotation` / `rotation` 子命令 |
| `export/maa.rs` | MAA JSON 导出 |

---

## 9. Agent 提示

- **跑模拟** → 默认 `plan`，需要纯排班时才用 `layout team-rotation`；不要用 `layout rotation` 或 `layout test`。
- **改中枢轮休 / 补位** → `schedule/team_rotation.rs` + `layout/assign.rs` + `search/control.rs`；不要把中枢重新放回 `pinned_assignment`。
- **改迷迭香/黑键同休** → `shift_bind.rs` + `team_rotation.rs`，不要改 `base_rotation.rs` 的 A-B-A 逻辑。
- **改但书 / 可露希尔 / 龙巫 meta 取舍** → `trade_segments.json` 的 `roles` + `search/role_pick.rs`；不要把 core priority 写回 fixed registry。
- **改 peak 编制** → 编排层 / `assign_shift`，见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)。
