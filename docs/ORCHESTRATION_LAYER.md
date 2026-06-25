# 编排层重构路线图（Orchestration Layer）

> **状态**：**Phase 0–3 / 5 已落地**（2026-06）；Phase 4 global effect 收拢进行中  
> **目标**：把「谁上哪个岗位」从 search/solve 评分里拆出来，统一到 **System → Plan → Execute**；L1–L3 求解器不改语义，只消费编制结果。  
> **背景讨论**：同房组合、跨房体系、global effect 三套入口搅在 `assign_shift` 里是乱源，不是机制太多。  
> **旧稿参考**：[plans/orchestration_engine_design.md](../plans/orchestration_engine_design.md)（阶段二/三「穷举方案 + DailyTotals 裁决」**不采用**；组合由 System 声明，不靠 search 发现）

---

## 1. 问题陈述

历史上 `layout/assign.rs` 的 `assign_shift` 同时承担了多层职责；当前主路径已收敛为 `build_plan -> execute_plan -> fill_greedy`，下表记录边界：

| 职责 | 应在哪 | 当前边界 |
|------|--------|------|
| 选跨站体系 / 固定 bond 进编 | **编排** | `layout/orchestrate::{build_plan, execute_plan}`；`claim_base_systems` 仅兼容 / 测试辅助 |
| 贸易核心优先 / 填散件第三人 | **role policy + 搜索（子集）** | `trade_segments.roles` + `search/role_pick.rs`；`assign_trade_remainder` / 制造 / 发电贪心填空房 |
| 算全局池 / 中枢注入 | **resolve** | `resolve_base` + `cross_facility` / `global_resource` |
| 算同房效率 / 产量 | **L1–L3 solve** | 只消费编制结果，不负责发现 meta 组合 |

该重构要避免的后果：组合表化后又被 search 打散（如黑键可露链 vs 但书链）、`flat_eff_hint` / 并站 solve 分等补丁继续扩散。

---

## 2. 三种耦合范围 → 三种职责（不混层）

游戏机制（见 `data/MECHANICS_REGISTRY.csv`）可按**耦合半径**分类：

| 范围 | CSV 特征 | 编排 / 运行时 |
|------|----------|----------------|
| **同房** | 「当与 X 在同一个贸易站」 | System 的 `bond` / `fixed` slot + L3 `shortcut` 回归锚点 |
| **跨房体系** | 中枢/宿舍 producer → 贸易/制造 consumer | **同一个 System**，多 `facility` slot + `trade_segments` producer 前提 |
| **全基建池** | 感知、人间烟火、木天蓼、虚拟电站 | **不是组合**；`resolve_base` → `GlobalResourcePool` + `cross_facility` |

**与 `OperatorTier` 枚举的对应关系**（`crates/infra-core/src/layout/tier.rs`）：

| 耦合范围 | OperatorTier | 分配阶段 |
|----------|-------------|----------|
| 跨房体系 | `CrossStation` | 第 1 轮 `select_registry_systems` |
| 同房 bond | `SameStation` | 第 2 轮 `select_registry_systems` |
| 散件效率工具人 | `Standalone` | 不在 registry；由 `try_filter_standalone` + `C(n,3)` 贪心 |

`base_systems.json` 中每个 System 通过 `"tier"` 字段声明归属，`select_registry_systems` 据此分两阶段贪心：
先跨站、后同站，`exclusive_group` 互斥态跨轮共享。

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

模块位置（当前）：

```
crates/infra-core/src/layout/orchestrate/
  mod.rs
  plan.rs      # AssignmentPlan, SlotFill, ActivatedSystem
  select.rs    # System 选型、tier 降级、exclusive_group
  execute.rs   # 落位：fixed / bond / core+segment / pick_one
```

`assign_shift` 当前主线为：seed → `build_plan` → `execute_plan` → producer/resolve → 发电 → 贸易 core role / plain → 制造贪心填充。贸易早于制造填充，避免但书、可露希尔、巫恋等核心和其工具人被制造站提前占用。

---

## 4. System 统一 schema（扩 `base_systems.json`）

**同站组合与跨站体系都是 System**，差别只在 `slots` 数量与 `prerequisites`。

### 4.1 `tier` — 三层分配优先级

`base_systems.json` 中每个 System 必须声明 `"tier"` 字段：

| 值 | 含义 | 分配阶段 |
|----|------|----------|
| `cross_station` | 跨站体系（slot 跨多设施） | `select_registry_systems` 第 1 轮 |
| `same_station` | 同站组合（slot 在同一设施内） | `select_registry_systems` 第 2 轮 |

两轮共享 `exclusive_group` 互斥态。未注册的散件干员属第三层 `Standalone`，不走 registry，由 `try_filter_standalone` + `C(n,3)` 贪心填充。

对应 Rust 枚举：`crates/infra-core/src/layout/tier.rs` 的 `OperatorTier`，同时标注到 `RegistrySystemClaim`、`ActivatedSystem` 和各设施 `PoolEntry` 的 `tier` 字段。

### 4.2 `fill_mode`

| 值 | 含义 | Executor |
|----|------|----------|
| `fixed` | 整包落位 | 直接 `set_room`（例：但书+伺夜+贝洛内） |
| `bond` | 二人锁死 + 第三人 | 固定 A+B，`pick_one` 填第三（例：德 E0+拉普兰德 **或** 能天使+蕾缪安；同干员不同 tier 须分叉 System） |
| `core` | 单人锚 + segment 池补满 | 仅用于未来非感知散件锚点；**黑键不走此路径** |
| `pick_one` | 列表选一 | 第一个可用干员 |
| `greedy` | Plain | `C(n,3)` 或发电 O(n)（制造仍全池穷举） |

`pick_one` 候选默认继承 slot 级 `"elite"` 要求；需要按候选区分精英化门槛时可写对象，例如 `{ "name": "海沫", "elite": 2 }`。对象候选还支持 `"max_elite"`，用于保留少量 E0-only / E1-only 历史锚点。

### 4.3 与现有文件的关系

| 文件 | 角色 |
|------|------|
| `data/base_systems.json` | System 目录：`tier`（`cross_station` / `same_station`）、priority、`exclusive_group`、slots |
| `data/trade_segments.json` | producer 条件 + consumer → `shortcut_id`；`roles` 声明贸易核心优先 fallback 链 |
| `data/trade_shortcuts.json` | L3 组合级 trade%/gold% 锚点 |
| `layout/system_integrity/` | **已迁出编排主路径**；迷迭香感知链待 Phase 4 `cross_facility` + 计算效率后进编 |

### 4.3 贸易 meta：跨站体系、同站锚点与核心优先

来源：`MECHANICS_REGISTRY.csv` 中「当与 X 在同一个贸易站」+ 公孙工具人表。  
`base_systems.json` 保留跨站体系和历史同站锚点；当前 `assign_shift` 主路径会跳过 `witch_long_beta`、`blackkey_closure`、企鹅、推王等低优先 registry 抢站条目，贸易余站改由 `data/trade_segments.json` 的 `roles` 执行核心优先。

#### 已落地的数据与运行时归属

| id / role | 类型 | L3 shortcut | 当前运行时 |
|-----------|------|-------------|------------|
| `docus_syracusa` / role `docus` | 跨站 meta + 但书核心优先 | `gsl_docus_syracusa` / `gsl_docus_solo` | 完整叙拉古链优先；缺伺夜/贝洛内时仍强制包含但书配最高可用工具人 |
| `closure` | 可露希尔核心优先 | `gsl_blackkey_closure` / `gsl_closure_*` | 强制包含可露希尔；优先黑键可露锚点，缺黑键仍保留可露 |
| `witch` | 巫恋核心优先 | `gsl_witch_*` | 强制包含精二巫恋；龙舌兰优先，裁缝 β / α / 空白第三人 fallback |
| `ling_jie_karlan` | control producer + L1 自然搜索 | `gsl_ling_jie_yaxin` 仅参考 | 只认领灵知 E2 中枢；精1孑由贸易搜索注入 |
| `vina_lungmen` / `penguin_*` | 低优先 bond / segment 锚点 | `gsl_vina_lungmen` / `gsl_penguin_*` | 保留 L3 锚点和旧测试；普通排班不早于但书/可露/巫恋核心抢站 |
| `rosemary_perception*` | **global effect** | — | 已移出编排；`assign_perception_producers` + scope=global |

#### 贸易核心 role 顺序

贸易金单余站按 `pick_trade_meta_then_plain` 尝试：

1. `docus`：完整叙拉古链优先；否则 `gsl_docus_solo`；否则仍只搜索包含但书的三人组；无但书则失败，不退化成“无但书 plain”。
2. `closure`：`gsl_blackkey_closure` 优先；否则 `gsl_closure_*`；否则包含可露希尔的最高可用三人组。
3. `witch`：`gsl_witch_*`；支持龙舌兰精二 + 裁缝 β / α / 空白第三人等 fallback。
4. plain：散件工具人三人组，且排除黑键与巫恋同房冲突。

这条顺序是核心优先策略，不是固定三人组优先级。完整叙拉古链仍是跨站 meta；八幡海铃、伺夜、贝洛内不是普通贸易散件。

#### Phase 2 待建（贸易 bond）

| System id | fill_mode | 同房 bond（CSV） | tier / 备注 | L3 shortcut |
|-----------|-----------|------------------|-------------|-------------|
| `penguin_texlap_e0` | bond | 德克萨斯 + 拉普兰德（恩怨 +65% 贸） | vault 确认德克萨斯 E2 不失去恩怨；不再限制 E0-only，id 保留兼容 | `gsl_penguin_texlap_e0` ✅ |
| `penguin_texangel_e2` | bond | **德克萨斯 E2** + 能天使（默契） | 德须 **精2**；第三人 `pick_one` | `gsl_penguin_texangel_e2` ✅ |
| `penguin_exusiai_lemuen` | bond | 能天使 + 蕾缪安 E2（相伴 +25%） | 第三人 `pick_one` | `gsl_penguin_exusiai_lemuen` ✅ |

**企鹅物流注意**：上是 **两套核**（德狼 vs 能蕾），不是一条 System；243 双贸通常只认领其一。选型用 `exclusive_group: penguin_meta` 或 priority，**不靠 search 发现**。  
德狼路线以 vault 为准：德克萨斯 E2 不会失去「恩怨」，因此德狼不再限制 E0-only；`penguin_texlap_e0` / `gsl_penguin_texlap_e0` 名称暂保留为兼容旧 id。

#### 一般不表化为 System（仍由 Plain search / tag 处理）

| 机制 | CSV | 原因 |
|------|-----|------|
| 新约能天使「同城加急单」 | 同站每名拉特兰 +15% | tag 叠层，非固定二人 bond |
| 维娜「外贸决议」 | 同站 GSG 干员 +10% | 已含于推王 fixed 三人组纸面 |
| 孑「市井之道」 | 站内含义依赖订单上限与技能顺序 | 灵知线由 L1 搜索自然上浮，非固定 trade slot / active L3 |

Producer 前提（跨房，非 global pool）：

- 叙拉古：`haru_e2_in_control`（八幡海铃 E2）
- 喀兰：`karlan_precision`（灵知 E2）
- 推王：`戴菲恩` E2 在中枢（运筹好手）

### 4.4 制造同房 bond（自动化链配套，Phase 2+）

`MECHANICS_REGISTRY.csv` 中「在同一个制造站」；与贸易 meta 并列登记，避免只改贸易漏制造。

| 组合 | CSV 技能 | 编排建议 | 状态 |
|------|----------|----------|------|
| 水月 E2 + 两名标准化 β | 意识协议 / 标准化 β | `same_station` 固定制造站，同站 meta；β 工具人用 `pick_one`，海沫单独要求 E2 | ✅ `standardization_mizuki` |
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
- [x] **已移除 / 不再存在**：`apply_blackkey_colocate_rule`、`assign_trade_meta`、`complete_trade_anchor_rooms`（旧黑键贸锚）
- [x] 贸易核心 role：`docus` / `closure` / `witch` 写入 `trade_segments.json`，由 `search/role_pick.rs` 统一执行
- [x] `assign_shift` 主路径跳过 `witch_long_beta`、`blackkey_closure`、企鹅、推王等旧 registry 早占站条目，改由 role policy 选择
- [x] **验收**：缺伺夜/贝洛内时但书仍进站；缺黑键时可露仍进站；缺裁缝 β 时巫恋走 α / blank fallback；小饼类账号保留但书、可露、龙巫

#### 巫恋 role

- **核心**：精二巫恋；龙舌兰精二优先。
- **fallback**：裁缝 β → 裁缝 α → 空白第三人；对应 `gsl_witch_long_beta` / `gsl_witch_long_alpha` / `gsl_witch_long_blank` 等。
- **编排**：不再把 `witch_long_beta` 当固定三人组早占站；由 role policy 在贸易余站搜索里强制包含巫恋。

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

- [x] α/β 从 peak 切半保留编排已认领贸易 meta；γ 走 `assign_team_gamma_half`（同样使用 docus → closure → witch → plain 的贸易 role 顺序）
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

- **改贸易 meta 进编** → 先读本文 + 改 `base_systems.json` / `trade_segments.json`，**不要**先改 `search/trade.rs` 打分。新增 System 时务必填写 `"tier"` 字段（`cross_station` / `same_station`）。
- **改机制数值** → 仍走 [AGENTS.md](../AGENTS.md) §5 分层（L1/L2/L3）。
- **改 global 池** → `cross_facility/` + `scope: global` atom，不动 assign。
- **验证模拟** → [AGENTS.md](../AGENTS.md) §6.2 `plan` 或 `layout team-rotation`。
