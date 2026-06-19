# 制造站域状态

> **勿按贸易站 L2/L3 假设改制造站。** 制造站无 `gold_flow` / `order_mechanic` / `trade_shortcuts`；求解为 L1 直通 `solve_manufacture`。  
> **搜索刻意全池 `C(n,3)`**：制造站无贸易式「金标组合」L3，穷举 + `composite` 评分是定稿设计，不是待补缺口。

## 域对比

| 维度 | 贸易站 | 制造站 |
|------|--------|--------|
| L1 | `trade/interpreter.rs` | `manufacture/interpreter.rs` |
| L2 域引擎 | `gold_flow`、`order_mechanic` | **无**（复杂机制仍在 L1 或待建域引擎） |
| L3 组合表 | `trade_shortcuts.json` | **无** |
| 池 | `pool/trade.rs` | `pool/manufacture.rs` |
| 搜索 | `search/trade.rs` | `search/manufacture.rs` |
| 求解 | `solve_trade_with_shift` | `solve_manufacture` |
| 排班 | `schedule/base_rotation.rs`（`assign_shift` 内制造产线） | ✅ `assign_shift`（高峰/恢复班覆盖） |
| CLI 回归 | `verify` + CSV | **无** dedicated verify case |

## 已实现

| 模块 | 职责 |
|------|------|
| `manufacture/input.rs` | `ManuOperator`、`ManuRoomInput`、`ManuLineScenario`、`ManuSearchRecipeMode` |
| `manufacture/interpreter.rs` | `apply_manu_phases`；`ManuContext` / `RecipeEff` / `RecipeLimit` |
| `manufacture/solver.rs` | `solve_manufacture`、`score_manu_composite`；按 `RecipeKind` 输出产能与仓库 |
| `pool/manufacture.rs` | 从 operbox 制造 roster 建池；跳过未建模 buff |
| `search/manufacture.rs` | C(n,3) 穷举；支持单配方 / 多产线 split（2 金 + 2 经验默认） |

### 制造 L1 与贸易 L1 的差异（摘要）

- 上下文追踪 **配方产能**（`RecipeEff`）与 **仓库上限**（`RecipeLimit`），非订单效率%。
- `ActiveRecipe` Condition 在制造站生效；贸易站用于订单种类。
- 无 `PeerAbsorb` 后的 `gold_flow` 挂钩。
- 共享同一 `skill_table.json`（制造 buff 与贸易 buff 同表不同 id）。
- **时间爬升**（芬/克洛丝/稀音/阿罗玛等）：`Action::AddEffRamp` → 纸面取 **20h 逐时效率算术平均**（见 `eff_ramp.rs`）；发电空构仍用 `shift_hours` 单点。

## CLI 入口

| 命令 | 说明 |
|------|------|
| `pool --manufacture --operbox <path>` | 制造池统计（**必须** operbox，无默认 roster） |
| `bench --operbox <path>` | 同时 bench 贸易 + 制造搜索 |
| `search` | 子命令内可触发制造搜索（见 `main.rs` `search_cmd`） |
| **`layout test`** | 默认调用 `assign_base_greedy` 宏观落位→ `resolve_base` → 制造搜索（含产线拆解） |
| **`layout team-rotation`** | αβγ ABC：三班均覆盖制造产线（现行） |
| **`layout rotation`** | ~~A-B-A~~ 已废弃 |

输出在 `infra-cli/output.rs` 的 `emit_bench` / pool 相关段。

## 数据

| 文件 | 状态 |
|------|------|
| `data/skill_table.json` | 制造 buff 与贸易 buff **共用**；`atoms: []` 仍表示委托（制造域引擎未建时跳过或 L2 占位） |
| `data/prts_manufacturing_skills.json` | PRTS 制造技能原文快照（核对用） |
| `data/operator_instances.json` | 干员 tier → buff_ids（与贸易共用） |
| `scripts/build_manufacturing_skill_table.py` | 制造技能表构建/校验 |

**无** `manufacture_shortcuts.json`、**无** `REGRESSION_CASES` 制造列。

## 全局资源 / 布局

制造求解可读 `TradeLayoutContext`（搜索时传入）：`ManuRecipeKinds`、`effective_power_station_count` 等来自 layout / 全局池快照。中枢编制见 `control::apply_control_to_layout`；资源注册见 `EFFECT_ATOM_DESIGN.md` §8.13。

`GlobalInject` phase 在贸易 L1 为空操作；制造 / 中控侧在 `control/interpreter.rs` 处理。

### 已闭环示例（怪猎 P0）

| 干员 | 技能 | 布局依赖 |
|------|------|----------|
| **泰拉大陆调查团** | 可靠的随从们 | `layout.global.Matatabi`（木天蓼 12 → 生产力 +17% 含 flat 5%） |
| （间接）**麒麟R夜刀** 精2 | 以身作则 | `global_inject` 全制造 +2%（`snhunt_elite2_baseline()`） |

评估怪猎制造搜索时应用 `TradeLayoutContext::snhunt_baseline()` 或 `snhunt_elite2_baseline()`，勿用默认 `search_baseline()`（无木天蓼）。

## 已知缺口（相对贸易站）

- [ ] 制造专用 L2 域引擎（若出现大量 `atoms: []` 委托）
- [ ] 制造 L3 组合表（若出现类似巫恋核的表化最优解）
- [ ] `verify` 制造回归 CSV + 夹具
- [x] **制造产线排班** — 已通过 `assign_shift`（`Peak` / `Recovery`）覆盖；`schedule_base_rotation_a_b_a` 含制造评分
- [ ] `市井之道产能耦合` 类问题在制造侧重算 score 时尚未完全对齐贸易站讨论（见设计文档 §九）
- [ ] 制造 `GlobalInject` 特定 buff（目前走 `control/interpreter.rs`，未单独拆制造侧 `global_inject` 阶段）

## 改制造站时推荐顺序

1. 本文 + `manufacture/interpreter.rs` 局部（结构同 [INTERNAL/TRADE_INTERPRETER.md](INTERNAL/TRADE_INTERPRETER.md)）
2. `types.rs` / `skill_table.json` / `operator_instances.json`
3. `manufacture/solver.rs` 与 `search/manufacture.rs` 评分
4. `cargo test -p infra-core`（制造相关 `mod tests`）
5. **不要**改 `trade/shortcut.rs` 或 `REGRESSION_CASES.csv` 除非同时动贸易站
