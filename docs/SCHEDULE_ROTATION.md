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
| 设施空转 | **禁止**（每班满编） | 允许恢复班降配 |
| 默认模拟 | ✅ [AGENTS.md](../AGENTS.md) §6.2 | ❌ 不要用 |

用户说「跑一遍模拟」「三班模拟」时，一律用 **`layout team-rotation`** + `--maa-out`，见 [INFRA_CLI.md](INFRA_CLI.md)。

---

## 2. ABC 轮换流程

```
peak = assign_shift_with_plan(Peak) → { assignment, plan }
shared = pinned_assignment(peak)     # 中枢/宿舍三班钉死
[h1, h2] = split_production_facilities
align_shift_binds(h1, h2)            # 迷迭香+黑键等同队
α = peak ∩ h1,  β = peak ∩ h2
γ = assign_team_gamma_half(h1) + assign_team_gamma_half(h2)  # plain 贸易，不重搜 meta

S1 (12h): shared + α(H1) + β(H2)   休息 γ
S2 (6h):  shared + β(H2) + γ(H1)   休息 α
S3 (6h):  shared + γ(H2) + α(H1)   休息 β
```

γ 替补贸易与 peak `assign_trade_remainder` 同路径（`trade_hit_ok_for_greedy`），制造/发电仍站绑定贪心。

实现：`crates/infra-core/src/schedule/team_rotation.rs`。

---

## 3. 班次绑定（shift_bind）

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

## 4. CLI 与 MAA

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

## 5. 与编排层的关系

- **单班编制**（peak/recovery）：`assign_shift` → 编排 `System → Plan → Execute`（见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)）。
- **多班轮换**：在 peak 编制之上切半 + γ 替补；**不**在编排层做 shift_bind。
- 迷迭香体系：**不进编排 execute**；感知链由 global effect + `assign_perception_producers` 处理（Phase 4）。

`TeamRotationReport.peak_plan` 携带完整 `AssignmentPlan`（JSON 可序列化）；text 输出打印已选体系与贸易 meta 房间。

---

## 6. 相关文件

| 文件 | 作用 |
|------|------|
| `schedule/team_rotation.rs` | ABC 主流程 |
| `schedule/shift_bind.rs` | 班次绑定定义与对齐 |
| `layout/assign.rs` | `assign_team_gamma_half`（γ plain 贸易） |
| `schedule/base_rotation.rs` | A-B-A legacy + `score_base_assignment`（ABC 复用评分） |
| `infra-cli/commands/layout.rs` | `team-rotation` / `rotation` 子命令 |
| `export/maa.rs` | MAA JSON 导出 |

---

## 7. Agent 提示

- **跑模拟** → `layout team-rotation`，不要用 `layout rotation` 或 `layout test`。
- **改迷迭香/黑键同休** → `shift_bind.rs` + `team_rotation.rs`，不要改 `base_rotation.rs` 的 A-B-A 逻辑。
- **改 peak 编制** → 编排层 / `assign_shift`，见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)。
