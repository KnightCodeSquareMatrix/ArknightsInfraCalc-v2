# 项目地图（Agent / 开发者入门）

> **新会话请先读 [AGENTS.md](../AGENTS.md)**，再读本文。机制细节见 [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md)，协作节奏见 [COLLAB_WORKFLOW.md](COLLAB_WORKFLOW.md)。大文件内部边界见 [INTERNAL/](INTERNAL/)。

**结构已定稿**：不再做大范围源码拆分；靠文档路由到正确函数段即可。

## 项目是什么

明日方舟**基建贸易站**效率求解器（v2 绿场重写）。给定干员练度（operbox）与场景假设（贸易站等级、赤金线数、布局 buff 等），计算同房三人组的订单效率、机制等效效率、单位产出，并支持穷举搜索与三班 A-B-A 轮换排班。

**当前范围**：贸易站为**主力**（L1+L2+L3+回归齐全）；制造站有 L1+搜索+bench；中控/全局资源 **P0 怪猎木天蓼链 + 宿舍人数链** 已闭环，其余 producer 见 §8.13、§九。

### 各域落地状态

| 域 | L1 | L2 | L3 | 搜索 | 排班 | CLI 回归 | 说明 |
|----|----|----|-----|------|------|----------|------|
| **贸易站** | ✅ | ✅ | ✅ | ✅ | ✅ 三班 A-B-A | ✅ | 主力 |
| **制造站** | ✅ | — | — | ✅ | — | — | 见 [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md) |
| **控制中枢** | ✅ | — | — | — | — | — | `control/`：木天蓼 producer、精2 全局注入、心情记账 |
| **全局资源** | 注册表 ✅ | 池 ✅ | — | — | — | — | P0：木天蓼 / 人间烟火（简化）/ 魔物料理（基准注入） |

域详情：**制造** → [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md)；**全局资源** → [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) §8.13。

**非目标**（由上层规划器负责）：心情排班、宿管恢复、全基建连班优化。见设计文档 §8.12。

**全基建单班进驻编制**（`BaseAssignment`、并行搜 + `used` 落位）：设计见 **[BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md)**（实现待落地）。

---

## 求解流水线（一眼看懂）

```
operbox / roster + operator_instances + skill_table
        ↓
  pool（可建模干员池，C(n,3) 组合基数）
        ↓
  search（穷举三人组，rayon 并行） 或  schedule（三班贪心逐站）
        ↓
  solve_trade_with_shift（单站核心）
        ├─ L1 interpreter   Phase 排序 → Selector/Condition/Action
        ├─ L2 gold_flow     赤金虚拟线链（进驻顺序状态机）
        ├─ L2 order_mechanic 订单分布 → 机制等效效率
        └─ L3 shortcut      trade_shortcuts.json 表化最优解（热路径 + 回归锚点）
        ↓
  unit_output             单位贸易量 / 无人机 / 日产量
```

**关键约定**：L1 代码**不认识干员名**，只认 `buff_id`。L2/L3 可按 `buff_id` 或组合类型分支。`skill_table` 中 `atoms: []` 表示**委托给域引擎**，不是未建模。

---

## 仓库目录

```
ArknightsInfraCalc-v2/
├── Cargo.toml              workspace：infra-core + infra-cli
├── README.md
├── AGENTS.md               Cursor / 新会话首读（链到本文）
├── docs/
│   ├── PROJECT_MAP.md      ← 本文
│   ├── EFFECT_ATOM_DESIGN.md   机制词汇、已建模干员、分层求解定稿
│   ├── MANUFACTURE_STATUS.md   制造站域状态（勿按贸易站假设改）
│   ├── BASE_ASSIGNMENT.md      全基建进驻编制（宏观排班）设计
│   ├── INFRA_CLI.md            infra-cli 模块职责与改动边界
│   ├── COLLAB_WORKFLOW.md      逐干员协作节奏与数据不变式
│   └── INTERNAL/               大文件内部地图（interpreter / shortcut）
├── crates/
│   ├── infra-core/         库：类型、解释器、求解、搜索、排班
│   └── infra-cli/          命令行：verify / pool / search / schedule / trade
├── data/                   机制注册表、干员实例、回归用例（运行时真相源）
├── scripts/                Python：数据校验、PRTS 快照、operbox 转换
└── tests/fixtures/         最小 JSON 夹具（如 closure_tier90）
```

---

## `infra-core` 模块索引

| 模块 | 文件 | 职责 |
|------|------|------|
| **types** | `src/types.rs` | `Phase` / `Selector` / `Action` / `Condition` / `EffectAtom` / `SkillDef` — JSON 与解释器共享的类型 |
| **tier** | `src/tier.rs` | `PromotionTier`（精0 / 精1+） |
| **skill_table** | `src/skill_table.rs` | 加载 `data/skill_table.json`；`data_path()` / `workspace_root()` |
| **instances** | `src/instances.rs` | `operator_instances.json`；`resolve_buff_ids`（含 stepwise 技能）；`buff_stem` |
| **roster** | `src/roster.rs` | 贸易站干员名单 CSV（`roster.csv` 等），按设施过滤 |
| **operbox** | `src/operbox.rs` | 玩家练度盒 JSON（拥有哪些干员、精英化等级） |
| **error** | `src/error.rs` | 统一 `Error` / `Result` |
| **pool** | `src/pool/trade.rs`、`pool/manufacture.rs` | 贸易/制造可求解池；跳过无绑定/未建模 buff |
| **search** | `src/search/trade.rs`、`search/manufacture.rs` | C(n,3) 穷举 + 评分（`TradeSearchHit` / `ManuSearchHit`） |
| **schedule** | `src/schedule/trade_rotation.rs` | 三班 A-B-A：`schedule_trade_rotation_a_b_a`，每班 3 站×3 人贪心 |
| **control** | `src/control/` | 中枢 `solve_control` → `apply_control_to_layout` 写回 layout 全局注入 |
| **global_resource** | `src/global_resource/` | `GlobalResourceKey`、`REGISTRY`、`CONVERSIONS`、`GlobalResourcePool` |
| **manufacture** | 见 [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md) | 制造 L1 + 求解 + 搜索（无 L2/L3） |
| **trade** | 见下表 | 贸易站求解核心 |

### `trade/` 子模块

| 文件 | 层级 | 职责 |
|------|------|------|
| `input.rs` | — | `TradeOperator` / `TradeRoomInput` / `TradeLayoutContext`（`search_baseline` / `snhunt_baseline` / `snhunt_elite2_baseline`） |
| `layout/` | — | `resolve_base`、`data/layout/*.json`、`snhunt_default_assignment()` |
| `interpreter.rs` | L1 | `TradeContext`、`apply_trade_phases`；按 Phase 执行 EffectAtom；挂钩 `gold_flow`。**内部地图**：[INTERNAL/TRADE_INTERPRETER.md](INTERNAL/TRADE_INTERPRETER.md) |
| `gold_flow.rs` | L2 | `apply_gold_flow_chain`：绮良/鸿雪/图耶等虚拟赤金线累加 |
| `order_mechanic.rs` | L2 | 订单 tag/分布 → `mechanic_equiv_eff_pct`（违约、裁缝、龙舌兰等） |
| `shortcut.rs` | L3 | 加载 `trade_shortcuts.json`；但书 solo / 灵孑银崖 / 巫恋组 / 可露希尔分档；同房互斥校验。**内部地图**：[INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md) |
| `solver.rs` | — | **`solve_trade` / `solve_trade_with_shift`**：串联 L1→L2→L3，输出 `TradeResult` |
| `unit_output.rs` | — | 单位产出、无人机折算、`TradeDailyYield` |

**改机制时最常打开的顺序**：`types.rs`（新词汇）→ `skill_table.json` → `interpreter.rs` 或 L2 引擎 → `shortcut.rs` / `trade_shortcuts.json` → `solver` 测试 → `REGRESSION_CASES.csv`。

---

## `infra-cli` 命令

| 命令 | 用途 |
|------|------|
| `verify --case <id>` / `--all` | 跑 `REGRESSION_CASES.csv` + `UNIT_OUTPUT_ANCHORS.csv` |
| `pool --trade` | 打印贸易站池统计与跳过原因 |
| `search trade [--roster] [--top N]` | 全池 C(n,3) 搜索 Top-K |
| `bench --operbox <path>` | 243c 基准布局 + operbox 贸易/制造搜索（**无**怪猎木天蓼；怪猎号见下） |
| **`layout test --layout <path> --operbox <path>`** | **自定义 `BaseBlueprint` + operbox 贸易/制造搜索（Agent 默认探测路径）** |
| `schedule rotation --operbox <path> [--layout-baseline] [--json]` | 三班 A-B-A 轮换报告 |
| `trade yield <fixture> [--level] [--shift]` | 单站产量探测（fixture 名见 `verify/fixtures.rs` 的 `unit_fixture`） |

**Agent 默认夹具**（无用户路径时）：`data/fixtures/243/layout.json` + `data/fixtures/243/operbox_full_e2.json` — 见 [AGENTS.md](../AGENTS.md) §6.2、[INFRA_CLI.md](INFRA_CLI.md)「自定义布局 + 练度盒测试」。

**模块职责（必读）**：[INFRA_CLI.md](INFRA_CLI.md) — 约定 `commands` / `verify` / `output` 分工，避免把机制或夹具塞回 `main.rs`。

### `infra-cli` 源码索引

| 路径 | 职责 |
|------|------|
| `src/main.rs` | 进程入口、子命令路由；`pool` / `search` / `schedule` / `trade` / `bench` 编排（待迁入 `commands/`） |
| `src/commands/layout.rs` | `layout test`：蓝图 JSON + operbox → `resolve_base` → 搜索 |
| `src/commands/verify.rs` | `verify` 子命令：遍历 CSV、断言、PASS/FAIL |
| `src/verify/cases.rs` | 加载 `REGRESSION_CASES.csv`、`UNIT_OUTPUT_ANCHORS.csv` |
| `src/verify/fixtures.rs` | 硬编码 `TradeRoomInput`（回归 + `trade yield`） |
| `src/output.rs` | CSV/文本/JSON 输出；**不含求解** |

回归夹具在 `verify/fixtures.rs`；CSV `operators` 列尚未驱动夹具选择（按 `expect_shortcut` / `case_id` 映射）。

### 回归夹具映射（`verify_cmd`）

| `expect_shortcut` / 条件 | 夹具函数 | 典型 case |
|--------------------------|----------|-----------|
| `gsl_witch_*` | `witch_fixture(shortcut_id, level)` | 巫恋核各档 |
| `gsl_ling_jie_yaxin` | `ling_jie_fixture` | 灵知 + 孑 + 银灰 + 喀兰工具人 |
| `gsl_docus_*` | `docus_fixture(case_id, level)` | 但书三人组 |
| `expect_shortcut` 为 closure 且已接线 | `closure_fixture(case_id, level)` | `reg_gsl_closure_tier90` 等 |
| 其他 | — | 打印 `fixture not wired` 并 skip |

`trade yield <fixture>` 用 `unit_fixture`：`closure_solo` / `docus_solo` / `witch_long_beta` 等（见 `fixtures.rs`）。

`UNIT_OUTPUT_ANCHORS.csv` 的 `fixture` 列驱动 `unit_fixture` 名。

---

## `data/` 文件职责

| 文件 | 角色 | 谁维护 |
|------|------|--------|
| **`skill_table.json`** | `buff_id` → EffectAtom 列表；空 `atoms` = 委托 L2 | Cursor（用户确认后） |
| **`operator_instances.json`** | `干员@tier_0` / `干员@tier_up` → `buff_ids`；干员归属唯一真相 | Cursor |
| **`trade_shortcuts.json`** | L3 组合表化最优解 + verify 锚点 | 双方 |
| **`REGRESSION_CASES.csv`** | CLI `verify` 用例：期望 trade%/gold%/shortcut_id | 双方 |
| **`UNIT_OUTPUT_ANCHORS.csv`** | 单位产出 / GSL 赤金锚点 | 双方 |
| **`prts_trade_skills.json`** / `.csv` / `_table.html` | PRTS 贸易站技能原文快照（核对用） | 脚本抓取 |
| **`roster.csv`** | 默认贸易站搜索名单 | 脚本 / 手工 |
| **`roster_gongsun.csv`** / **`roster_xlsx.csv`** | 公孙长乐等扩展名单 | 导入 |
| **`data/fixtures/243/`** | **243 标准测试样例**：`layout.json` + `operbox_full_e2.json` + `schedule_export.json` | 夹具 |
| **`operbox_gongsun.json`** | 练度盒样例（较小子集） | 脚本 / 测试 |
| **`data/layout/243_use_this_.json`** | 公孙 243 事实蓝图（2 金贸）；同 `fixtures/243/layout.json` | 模板 |
| **`data/layout/243c.json`** | 旧版 243c（3 贸易：2 金 + 1 源石）；怪猎 `snhunt` 等同结构 | 模板 |
| **`data/layout/snhunt.json`** | 怪猎评估蓝图（物理同 243c；木天蓼靠中枢编制） | 模板 |
| **`MECHANICS_REGISTRY.csv`** | 全基建机制归档（727 条）；贸易站核对**不再依赖** | 归档 |
| **`需要完成的干员建模.md`** | 未完全建模干员清单与近期落地记录 | 维护 |
| **`data/tags/*.csv`** | 干员 tag（阵营等），供 Selector 引用 | 数据 |

---

## `scripts/` 脚本

| 脚本 | 用途 |
|------|------|
| `build_skill_table.py` | 构建/校验 skill_table；pilot 干员硬失败 |
| `check_trade_roster.py` | roster 与 skill_table / instances 一致性 |
| `audit_trade_buffs.py` | buff 覆盖审计 |
| `audit_trade_skill_rename.py` | 技能重命名审计 |
| `merge_skill_table.py` / `merge_closure_skills.py` | 合并技能表片段 |
| `build_roster_from_operbox.py` | 从 operbox 生成 roster |
| `xlsx_to_operbox.py` / `inspect_xlsx_operators.py` | 练度表 xlsx → operbox JSON |
| `save_prts_trade_table.py` | 抓取 PRTS 贸易站表格 |
| `verify_regression.py` | Python 侧回归（可选，Rust verify 为主） |
| `fix_trade_test_literals.py` | 测试字面量修复辅助 |

---

## 测试在哪里

- **单元测试**：各模块 `mod tests`（`cargo test -p infra-core`），重点在 `interpreter`、`solver`、`shortcut`、`gold_flow`、`pool`、`schedule`。
- **回归**：`infra-cli verify` + CSV；夹具 JSON 在 `tests/fixtures/`。
- **不变式**：`skill_table.id` 必须等于解包 `buff_id`；原文只信 `prts_trade_skills.json`。

---

## 常见任务 → 打开哪里

| 任务 | 先看 | 可能还要改 |
|------|------|------------|
| 新增/修改干员技能 | `EFFECT_ATOM_DESIGN.md` §四、`prts_trade_skills.json` | `skill_table.json`、`operator_instances.json`、L1/L2 引擎 |
| 新 Selector/Action | `types.rs` | `interpreter.rs`、设计文档 §三 |
| 赤金线链式机制 | `gold_flow.rs` | `skill_table` 空 atoms 注册 |
| 订单违约/裁缝/特别订单 | `order_mechanic.rs` | `shortcut.rs`、`trade_shortcuts.json` |
| 组合表化（巫恋/可露希尔档） | `trade_shortcuts.json` | `shortcut.rs`、`REGRESSION_CASES.csv`、`verify/fixtures.rs` |
| 搜索变慢/评分不对 | `search/trade.rs` | `solver.rs` 的 score 逻辑 |
| 三班轮换 | `schedule/trade_rotation.rs` | `operbox` 数据、`TradeSearchOptions` |
| 全基建单班进驻编制 | [BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md) | `layout/assign`（待实现）、`layout test --assignment` |
| 单位产出/无人机 | `unit_output.rs` | `UNIT_OUTPUT_ANCHORS.csv`、`verify/fixtures.rs` |
| 改 CLI 结构 / 回归夹具放哪 | [INFRA_CLI.md](INFRA_CLI.md) | 勿在 `main.rs` 堆新夹具 |
| L1 Phase / Condition 局部改 | [INTERNAL/TRADE_INTERPRETER.md](INTERNAL/TRADE_INTERPRETER.md) | 通读 interpreter 全文 |
| L3 匹配 / 同房互斥 | [INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md) | 只改 JSON 不改 matcher |
| 制造站搜索 / 池 | [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md) | `trade/shortcut.rs` |
| 中枢 / 全局资源 | `control/`、`global_resource/`、`layout/resolve.rs` | `EFFECT_ATOM_DESIGN.md` §4.8–4.12、§8.13 |
| 怪猎账号 / 木天蓼链 | `snhunt_baseline()`、`data/layout/snhunt.json` | 泰拉调查团、火龙S黑角、麒麟R夜刀 |
| 宿舍人数链（黑键/乌有/铎铃） | `DEFAULT_DORM_OCCUPANT_COUNT`、`layout/resolve.rs` | `EFFECT_ATOM_DESIGN.md` §4.8–4.10 |
| 导入玩家练度 | `operbox.rs`、`xlsx_to_operbox.py` | operbox JSON |
| 数据一致性报错 | `check_trade_roster.py`、`instances.rs` | roster / instances / skill_table |

---

## 新 Agent 推荐阅读顺序

1. **[AGENTS.md](../AGENTS.md)**（含 **§4 协作工序**）→ **本文**
2. [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) **§一、§八**（原则 + 三层架构）
3. 若改干员：[AGENTS.md §4](../AGENTS.md) + [COLLAB_WORKFLOW.md](COLLAB_WORKFLOW.md) + [需要完成的干员建模.md](../需要完成的干员建模.md) 定稿案例
4. 若改求解：`trade/solver.rs`（50 行内看清调用链）→ [INTERNAL/](INTERNAL/) 定位 L1/L2/L3 函数段
5. 若改制造：[MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md)
6. 若改数据：`data/skill_table.json` 一条样例 + `operator_instances.json` 对应干员

**不必通读**：`interpreter.rs` / `output.rs` 全文（用 INTERNAL 地图或 `emit_*` 名定位）、`MECHANICS_REGISTRY.csv`、PRTS HTML 快照、xlsx 练度表二进制。

---

## 依赖与技术栈

- Rust 2021 workspace，`serde`/`serde_json`/`csv`/`rayon`/`thiserror`
- 无数据库、无 Web 服务；纯本地计算 + JSON/CSV 数据
- 旧仓库 `ArknightsInfraCalc - 副本` 仅归档参考，**不迁移**其 Rust 求解器

---

## 相关文档

| 文档 | 内容 |
|------|------|
| [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) | EffectAtom 模型、词汇表、已建模干员、分层求解、排班与产量层 |
| [COLLAB_WORKFLOW.md](COLLAB_WORKFLOW.md) | 逐干员协作五步、数据不变式、验证命令 |
| [INFRA_CLI.md](INFRA_CLI.md) | CLI 分层原则、`commands` / `verify` / `output` 职责 |
| [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md) | 制造站实现范围与缺口 |
| [INTERNAL/](INTERNAL/) | `interpreter` / `shortcut` 大文件内部地图 |
| [AGENTS.md](../AGENTS.md) | Cursor 新会话首读、不变式、验证命令 |
| [README.md](../README.md) | 项目原则摘要与快速命令 |
