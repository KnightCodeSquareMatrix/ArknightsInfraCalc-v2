# 编排层重构路线图（Orchestration Layer）

> **状态**：**下一步主工程**（2026-06）  
> **目标**：把「谁上哪个岗位」从 search/solve 评分里拆出来，统一到 **System → Plan → Execute**；L1–L3 求解器不改语义，只消费编制结果。  
> **背景讨论**：同房组合、跨房体系、global effect 三套入口搅在 `assign_shift` 里是乱源，不是机制太多。  
> **旧稿参考**：[plans/orchestration_engine_design.md](../plans/orchestration_engine_design.md)（阶段二/三「穷举方案 + DailyTotals 裁决」**不采用**；组合由 System 声明，不靠 search 发现）

---

## 1. 问题陈述

当前 `layout/assign.rs` 的 `assign_shift` 同时承担：

| 职责 | 应在哪 | 现状 |
|------|--------|------|
| 选体系 / 固定组合进编 | **编排** | 部分在 `claim_base_systems`，部分在 `C(n,3)`+solve |
| 填散件第三人 | **搜索（子集）** | 与 meta 组合混用全池 search |
| 算全局池 / 中枢注入 | **resolve** | 与 `cross_facility`、`resolve.rs` 硬编码并存 |
| 算同房效率 / 产量 | **L1–L3 solve** | 被误用于「要不要并站 / 要不要某组合」 |

后果：组合表化了仍被 search 打散（如黑键可露链 vs 但书链）、`flat_eff_hint` / 并站 solve 分等补丁越来越多。

---

## 2. 三种耦合范围 → 三种职责（不混层）

游戏机制（见 `data/MECHANICS_REGISTRY.csv`）可按**耦合半径**分类：

| 范围 | CSV 特征 | 编排 / 运行时 |
|------|----------|----------------|
| **同房** | 「当与 X 在同一个贸易站」 | System 的 `bond` / `fixed` slot + L3 `shortcut` 回归锚点 |
| **跨房体系** | 中枢/宿舍 producer → 贸易/制造 consumer | **同一个 System**，多 `facility` slot + `trade_segments` producer 前提 |
| **全基建池** | 感知、人间烟火、木天蓼、虚拟电站 | **不是组合**；`resolve_base` → `GlobalResourcePool` + `cross_facility` |

**原则**

- **编排不算效率**：不调用 `solve_trade_with_shift` 决定 meta 组合。
- **L3 表 = 组合级硬编码效率**（工具人表）；search 只填 `pick_one` / Plain 站 `greedy`。
- **global effect 不参与进编**：编制定完后再 `resolve` 写池。

---

## 3. 目标流水线

```
assign_shift()
  └─ orchestrate::build_plan(operbox, blueprint, mode) → AssignmentPlan
       └─ orchestrate::execute_plan(plan) → BaseAssignment
            └─ fill_greedy(remaining empty rooms)   // 仅 Plain 贸易 / 制造 / 发电

resolve_base(assignment)   // 与 assign 分离，CLI 评分 / verify 才走
  ├─ control → GlobalInjectManifest
  ├─ cross_facility → GlobalResourcePool
  └─ per-room solve（L1 → L3 → L2 → production）
```

模块位置（计划）：

```
crates/infra-core/src/layout/orchestrate/
  mod.rs
  plan.rs      # AssignmentPlan, SlotFill, ActivatedSystem
  select.rs    # System 选型、tier 降级、exclusive_group
  execute.rs   # 落位：fixed / bond / core+segment / pick_one
```

`assign_shift` 瘦身为：seed → `build_plan` → `execute_plan` → `fill_greedy` → 发电/制造/宿舍等现有步骤（逐步迁入 execute）。

---

## 4. System 统一 schema（扩 `base_systems.json`）

**同站组合与跨站体系都是 System**，差别只在 `slots` 数量与 `prerequisites`。

### 4.1 `fill_mode`

| 值 | 含义 | Executor |
|----|------|----------|
| `fixed` | 整包落位 | 直接 `set_room`（例：但书+伺夜+贝洛内） |
| `bond` | 二人锁死 + 第三人 | 固定 A+B，`pick_one` 填第三（例：德 E0+拉普兰德 **或** 能天使+蕾缪安；同干员不同 tier 须分叉 System） |
| `core` | 单人锚 + segment 池补满 | 仅用于未来非感知散件锚点；**黑键不走此路径** |
| `pick_one` | 列表选一 | 第一个可用干员 |
| `greedy` | Plain | `C(n,3)` 或发电 O(n)（制造仍全池穷举） |

### 4.2 与现有文件的关系

| 文件 | 角色 |
|------|------|
| `data/base_systems.json` | System 目录：priority、`exclusive_group`、slots、`fill_mode` |
| `data/trade_segments.json` | producer 条件 + consumer → `shortcut_id` |
| `data/trade_shortcuts.json` | L3 组合级 trade%/gold% 锚点 |
| `layout/system_integrity/` | **已迁出编排主路径**；迷迭香感知链待 Phase 4 `cross_facility` + 计算效率后进编 |

### 4.3 贸易 meta：优先注册的组合（工具人表）

来源：`MECHANICS_REGISTRY.csv` 中「当与 X 在同一个贸易站」+ 公孙工具人表。  
`meta_chain` 互斥组内四选一：叙拉古 / 喀兰 / 推王 / 怪猎（见 `base_systems.json` `exclusive_group`）。

#### 已落地或数据齐套

| System id | fill_mode | 同房 bond（CSV 技能） | L3 shortcut | 状态 |
|-----------|-----------|----------------------|-------------|------|
| `docus_syracusa` | fixed | 贝洛内↔伺夜（未偿还的债务） | `gsl_docus_syracusa` | ✅ registry + shortcut；execute 收编中 |
| `ling_jie_karlan` | fixed | 孑+银灰+喀兰工具人（无二人 bond） | `gsl_ling_jie_yaxin` | ✅ registry + segment |
| `witch_long_beta` | **fixed（定稿）** | 巫恋+龙舌兰固定核；第三人裁缝β `pick_one` | `gsl_witch_long_beta` | ✅ **最终版**：registry fixed；**无** `meta_chain`；243 与但书链双贸共存 |
| `vina_lungmen` | fixed | 摩根↔推王（帮派指南针，站内 GSG tag） | `gsl_vina_lungmen` | ✅ registry + shortcut + segment |
| `blackkey_closure` | L3 锚 | 黑键+可露希尔+挂件（**不进编**；贪心 + `gsl_blackkey_closure` 打分） | `gsl_blackkey_closure` | ✅ shortcut + segment |
| `rosemary_perception*` | **global effect** | 感知 producer 落位 + `cross_facility` 算 layout → 贪心选型 | — | ✅ 已移出编排；`assign_perception_producers` + scope=global |

#### Phase 2 待建（贸易 bond）

| System id | fill_mode | 同房 bond（CSV） | tier / 备注 | L3 shortcut |
|-----------|-----------|------------------|-------------|-------------|
| `penguin_texlap_e0` | bond | **德克萨斯 E0** + 拉普兰德（恩怨 +65% 贸） | 德须 **精0** 档；`max_elite: 1` | `gsl_penguin_texlap_e0` ✅ |
| `penguin_texangel_e2` | bond | **德克萨斯 E2** + 能天使（默契） | 德须 **精2**；第三人 `pick_one` | `gsl_penguin_texangel_e2` ✅ |
| `penguin_exusiai_lemuen` | bond | 能天使 + 蕾缪安 E2（相伴 +25%） | 第三人 `pick_one` | `gsl_penguin_exusiai_lemuen` ✅ |

**企鹅物流注意**：上是 **两套核**（德狼 vs 能蕾），不是一条 System；243 双贸通常只认领其一。选型用 `exclusive_group: penguin_meta`（待加）或 priority，**不靠 search 发现**。  
德 E0「恩怨」与德 E2「默契」是同一干员的不同 tier 路线，必须在 `select.rs` 按 `operbox.elite_of("德克萨斯")` 分叉，不可混写成单一 bond。

#### 一般不表化为 System（仍由 Plain search / tag 处理）

| 机制 | CSV | 原因 |
|------|-----|------|
| 新约能天使「同城加急单」 | 同站每名拉特兰 +15% | tag 叠层，非固定二人 bond |
| 维娜「外贸决议」 | 同站 GSG 干员 +10% | 已含于推王 fixed 三人组纸面 |
| 孑「市井之道」 | 站内含义依赖订单分布 | L2 `order_mechanic`，非进编 bond |

Producer 前提（跨房，非 global pool）：

- 叙拉古：`haru_e2_in_control`（八幡海铃 E2）
- 喀兰：`karlan_precision`（灵知 E2）
- 推王：`戴菲恩` E2 在中枢（运筹好手）

### 4.4 制造同房 bond（自动化链配套，Phase 2+）

`MECHANICS_REGISTRY.csv` 中「在同一个制造站」；与贸易 meta 并列登记，避免只改贸易漏制造。

| 组合 | CSV 技能 | 编排建议 | 状态 |
|------|----------|----------|------|
| 阿兰娜 E2 + 温米 | 「搭把手！」 | 自动化金线固定 slot 或 `bond` + 第三人贪心 | ⚠️ shortcut 待补；registry 延至 Phase 3（避免抢公孙金线制造位） |
| Miss.Christine E2 + 酒神 | 盛餐的回报 |  niche；低优先 | ❌ |
| 怒潮凛冬 E2 + 乌萨斯学生自治团 | 情同手足 | tag 同房加成；可并进乌萨斯制造 meta | ❌ |

### 4.5 跨设施落位前提（非同房 bond，但影响 Plan）

| 干员 | 条件 | 影响 |
|------|------|------|
| 烈夏 E2 | **古米在贸易站**（患难拍档） | 制造站选人时须保留贸易古米位 |
| 清流 E1 | 每贸易站 → 当前制造站金 +20% | 自动化组已含；非 bond |
| 戴菲恩 E2 | 中枢 producer | 推王组 `prerequisites` |

---

## 5. 实施阶段

### Phase 0 — 模块边界 + Plan 类型

- [x] 新建 `layout/orchestrate/`，定义 `AssignmentPlan` / `SlotFill`
- [x] `assign_shift` 调用 `build_plan` → `execute_plan`（行为可先等价迁移）
- [x] **验收**：243 E2 现有集成测仍绿

### Phase 1 — System 选型（select）

- [x] 合并 `claim_base_systems` → `select.rs`（`select_registry_systems`）
- [x] 支持 `exclusive_group`、德克萨斯 E0/E2 企鹅物流 tier 分叉
- [x] **迷迭香感知链移出编排**（待 Phase 4 global effect 后按计算效率进编）
- [ ] **不实现**「多方案穷举 + DailyTotals 裁决」

### Phase 2 — 组合数据

- [x] 补 `trade_shortcuts.json` / `trade_segments.json` / `shortcut.rs` consumer
- [x] §4.3 贸易 meta 待建行齐套（企鹅三路 + 推王 shortcut + 黑键 segment）
- [ ] §4.4 制造 bond：阿兰娜+温米（延至 Phase 3 `bond` execute，避免破坏公孙金线回归）
- [x] **验收**：每个 System 一条 golden test（plan → assignment 快照 + 预期 `shortcut_id`）

### Phase 3 — Executor（execute）

- [x] registry `fixed` / `bond` 落位（`execute_plan`）；贸易余站 `assign_trade_remainder` 贪心
- [x] **删除**：`apply_blackkey_colocate_rule`、`assign_trade_meta`、`complete_trade_anchor_rooms`（黑键贸锚）
- [x] 黑键：感知链算 layout → 散件进 `C(n,3)`；`gsl_blackkey_closure` 仅 L3 打分
- [x] **巫恋组定稿**：`witch_long_beta` registry fixed；不做 `trade_role` / 多 System 变体
- [ ] `trade_role` / `role_pick`（仅余未进 registry 的散件；巫恋已排除）
- [x] **验收**：243 双贸 = 但书链 + 巫恋链（或黑键可露贪心）分站，不靠并站 patch

#### 巫恋组（定稿）

- **固定核**：精二巫恋 + 精二龙舌兰 + 裁缝β第三人（`pick_one` 卡夫卡/柏喙/明椒/折光）。
- **编排**：仅 `witch_long_beta`；无 `exclusive_group`，与但书链双贸共存。
- **L3**：进编锚 `gsl_witch_long_beta`；`gsl_witch_long_alpha` 等仅 verify / `classify_witch_room`，不进编。

### Phase 4 — global effect 收拢（与编排并行）

- [x] 感知 producer（爱丽丝/车尔尼/絮雨）`scope: global` atom → `cross_facility`
- [x] 删 `resolve.rs` 的 `apply_perception_producers` 硬编码
- [x] 制造房 `room_layout_for_manu` 声明式扣回 scope=Global atom
- [x] assign 层 `assign_perception_producers`：堆感知源后 resolve → 贪心消费 layout
- [ ] `skill_table` 其余干员 atom 标 `scope: global`（乌有人间烟火等）
- [ ] 删 `resolve.rs` 其余 `apply_*` 硬编码
- [ ] `GlobalInject` 留 `control/interpreter`，供 segment producer 读取
- [ ] 见 [INTERNAL/CROSS_FACILITY.md](INTERNAL/CROSS_FACILITY.md)、[EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) §九
- [x] 迷迭香 `shift_bind`：`schedule/shift_bind.rs` + `team_rotation`（见 [SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md)）

### Phase 5 — team-rotation 对齐

- [x] α/β 从 peak 切半保留编排已认领贸易 meta；γ 走 `assign_team_gamma_half`（plain 贸易，不重搜 meta/但书置顶）
- [x] `assign_shift_with_plan` + `TeamRotationReport.peak_plan` 供轮换层只读编排计划

---

## 6. 控 bug 策略

1. **编排单测不调 solve**；shortcut 单测才调 `resolve_trade_shortcut`。
2. **每个 System 一条 golden test**（`operbox_full_e2` 或最小 roster）。
3. 端到端：`layout team-rotation` + [fixtures/243](fixtures/243/README.md)。
4. 迁移期可用环境变量/feature 对比旧 `assign_shift` 总分（短期）。

---

## 7. 明确不做

- 用 `C(n,3)` + solve 分「发现」已知 meta 组合。
- 在编排层重复实现 global pool 逻辑（归 `cross_facility`）。
- 新 repo / 全量 v3 重写 L1–L3（机制回归成本过高）。
- `orchestration_engine_design.md` 中的全编制笛卡尔积 + 评分选优（组合已表化）。

---

## 8. 相关文档

| 文档 | 内容 |
|------|------|
| [BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md) | 现行单班编制流水线 |
| [SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md) | αβγ ABC 轮换、shift_bind、A-B-A 废弃说明 |
| [SYSTEM_CHAINS.md](SYSTEM_CHAINS.md) | 迷迭香/自动化/推王等体系链参考 |
| [INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md) | L3 匹配与 segment 注册表 |
| [INTERNAL/CROSS_FACILITY.md](INTERNAL/CROSS_FACILITY.md) | global atom 编排（resolve 内） |
| [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) §九 | AtomScope Room vs Global |

---

## 9. Agent 协作提示

- **改贸易 meta 进编** → 先读本文 + 改 `base_systems.json` / `trade_segments.json`，**不要**先改 `search/trade.rs` 打分。
- **改机制数值** → 仍走 [AGENTS.md](../AGENTS.md) §4 分层（L1/L2/L3）。
- **改 global 池** → `cross_facility/` + `scope: global` atom，不动 assign。
- **验证模拟** → [AGENTS.md](../AGENTS.md) §6.2 `layout team-rotation`。
