# Agent 引导（Cursor / 新会话首读）

> 本仓库规模已定型，**不再做大范围文件拆分**。请靠文档路由到正确模块，局部阅读即可。

## 1. 必读顺序

1. **[docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)** — 目录、模块索引、域状态、常见任务路由
2. 改机制 → 本文 **§4 协作工序** + **[docs/EFFECT_ATOM_DESIGN.md](docs/EFFECT_ATOM_DESIGN.md) §一、§八** + **[docs/COLLAB_WORKFLOW.md](docs/COLLAB_WORKFLOW.md)**
3. 改 CLI → **[docs/INFRA_CLI.md](docs/INFRA_CLI.md)**（禁止在 CLI 写求解公式）
4. 大文件内部边界 → **[docs/INTERNAL/](docs/INTERNAL/)**

## 2. 分层约束（违反即错）

| 层 | 模块 | 约束 |
|----|------|------|
| **L1** | `trade/interpreter.rs`、`manufacture/interpreter.rs` | 只认 `buff_id`，不认识干员名 |
| **L2** | `gold_flow.rs`、`order_mechanic.rs` | 机制域最优解；`skill_table` 空 `atoms` = 委托 |
| **L3** | `shortcut.rs` + `trade_shortcuts.json` | 组合表化；热路径 + 回归锚点 |
| **CLI** | `infra-cli` | 编排 + 输出 + 回归；机制在 `infra-core` |

求解入口：`trade/solver.rs` 的 `solve_trade_with_shift`（约 50 行看清 L1→L2/L3 调用链）。

## 3. 常见任务 → 先看哪里

| 任务 | 文档 / 文件 |
|------|-------------|
| 新 Action / Condition / Selector | `types.rs` → `EFFECT_ATOM_DESIGN.md` §三 |
| L1 Phase 分发 / 效率叠加 | [docs/INTERNAL/TRADE_INTERPRETER.md](docs/INTERNAL/TRADE_INTERPRETER.md) |
| 赤金虚拟产线 | `trade/gold_flow.rs` |
| 订单 tag / 违约 / 裁缝 | `trade/order_mechanic.rs` |
| L3 但书 / 巫恋 / 可露希尔 / 灵孑银崖 | [docs/INTERNAL/SHORTCUT_MATCHING.md](docs/INTERNAL/SHORTCUT_MATCHING.md) |
| 孑精0 vs 精1 / 灵知喀兰跨设施 | `需要完成的干员建模.md` §一；`seed_karlan_precision`、`global_resource/inject.rs` |
| 同房互斥 | `shortcut.rs`：`trade_station_exclusive_violation` |
| 全局资源 / 中枢注入 | `global_resource/`、`control/`、`layout/resolve.rs`；§4.8–4.12、§8.13 |
| 怪猎木天蓼链（调查团 / 火龙S黑角 / 麒麟R夜刀） | `snhunt_baseline()`、`data/layout/snhunt.json`；≠ 三星黑角/夜刀 |
| 制造站（勿按贸易站假设改） | [docs/MANUFACTURE_STATUS.md](docs/MANUFACTURE_STATUS.md) |
| 回归夹具 | `infra-cli/src/verify/fixtures.rs` + `PROJECT_MAP.md` 夹具表 |
| **自定义布局 + operbox 探测** | **`layout test`** — 见 [INFRA_CLI.md](docs/INFRA_CLI.md)「自定义布局 + 练度盒测试」；**不要**用 `bench` 代替 |
| **Agent 默认测试夹具（243 + 全精2）** | **`data/fixtures/243/layout.json`** + **`data/fixtures/243/operbox_full_e2.json`** — 见本文 **§6** |
| **全基建进驻编制 / 宏观排班** | [docs/BASE_ASSIGNMENT.md](docs/BASE_ASSIGNMENT.md)（`assign_base_greedy` 已落地；`layout test` 默认调用） |
| 数据一致性 | `scripts/check_trade_roster.py`、`instances.rs` |

## 4. 协作工序（改机制按层走）

新干员 / 新技能协作时，**分段通读**下列层即可；不必 grep 碎片 patch。完整样本（跨设施 + L3 锚 + 搜索绑定）：[需要完成的干员建模.md](需要完成的干员建模.md) §孑/灵知喀兰。

### 4.1 数据层（改干员必先动）

| 文件 | 反复要想的问题 |
|------|----------------|
| `data/operator_instances.json` | 干员 @tier 绑哪个 `buff_id`？`stepwise` 吗？`tags`（如 `cc.g.karlan`）对吗？ |
| `data/skill_table.json` | 这个 buff 的 `atoms` 怎么拆 Phase/Selector/Action？还是 `[]` 委托 L2？ |
| `data/trade_shortcuts.json` | 固定组合要不要 L3 锚？纸面 L1 和工具人表差多少？ |

不变式见本文 §7。

### 4.2 L1 解释器（机制「怎么算」）

| 模块 | 何时改 |
|------|--------|
| `types.rs` | 新 Selector / Action / Condition / Phase |
| `trade/interpreter.rs` | 贸易站相位、上限重算、selector 语义（如 OrderCount） |
| `control/interpreter.rs` | 中枢 GlobalInject、状态写入 |
| `manufacture/interpreter.rs` | 制造站（规则与贸易不同，勿混用） |
| `global_resource/inject.rs` | 跨设施注入 manifest（如灵知 `record_karlan_precision`） |

反复要想：相位顺序、`recompute_limit` 时机、跨房效果是在 control 写 manifest 还是在 trade `seed_*` 里落地。

### 4.3 L2 域引擎（复杂机制别硬塞 L1）

| 模块 | 典型内容 |
|------|----------|
| `trade/gold_flow.rs` | 鸿雪/图耶/绮良等虚拟产线 |
| `trade/order_mechanic.rs` | 违约、裁缝、特别订单 → 等效 trade% |
| `trade/unit_output.rs` | 纸面效率 × 单位产出 → score（市井耦合等待办见设计文档 §九） |

决策：能表化成 atom → L1；要订单分布/产能闭环 → L2；算不准或固定最优 → L3。

### 4.4 L3 短路 + 搜索 / 排班

| 模块 | 反复要想 |
|------|----------|
| `trade/shortcut.rs` | 新组合怎么 `match_*`？要不要 `global_inject` 上下文？ |
| `pool/trade.rs` | 建池、编译 atoms、孑 E0 降级、`karlan_precision_active` |
| `search/trade.rs` | 三人组搜索、互斥预筛 |
| `layout/resolve.rs` | 蓝图 → layout、control 注入、trade 房上下文 |
| `layout/assign.rs` / `schedule/trade_rotation.rs` | 宏观排班：谁强制、谁贪心 |

制造站搜索**刻意全池 `C(n,3)`**，无贸易式 L3 金标组合表 — 见 [MANUFACTURE_STATUS.md](docs/MANUFACTURE_STATUS.md)。

### 4.5 求解入口与验证

| 文件 | 作用 |
|------|------|
| `trade/solver.rs` | L1 → L3? → L2 → production；改机制最后要对这里 |
| 各模块 `mod tests` | 机制单测；`solver.rs` 集成测 |

协作固定顺序：**加/改 test → `cargo test -p infra-core` → 必要时 `verify --all`**（见 §6）。

### 4.6 文档（避免下次会话重讲）

| 文件 | 写什么 |
|------|--------|
| `需要完成的干员建模.md` | 谁还没完全建模、固定搭配、域策略 |
| `docs/EFFECT_ATOM_DESIGN.md` | atom 设计、§九 待办 |
| `docs/INTERNAL/SHORTCUT_MATCHING.md` | L3 匹配优先级 |

### 4.7 决策树（加干员前先问）

```
新干员/新技能
  ├─ 只影响同房 trade%？ → skill_table + trade/interpreter
  ├─ 只影响同房配方产能/仓库？ → skill_table + manufacture/interpreter（无 L3）
  ├─ 时间爬升（芬/克洛丝等）？ → eff_ramp.rs + skill_table AddEffRamp
  ├─ 跨设施（中枢→贸易/制造）？ → control + inject manifest + trade/manu seed
  ├─ 赤金/订单分布？ → gold_flow / order_mechanic
  ├─ 固定最优组合、L1 难算准？ → trade_shortcuts + shortcut.rs
  ├─ 影响 search/轮换/编制默认？ → pool + assign / trade_rotation
  └─ 文档 + 单测 +（可选）verify 夹具
```

加干员时先问：**数据有了吗？L1 够吗？要不要 L2/L3？制造仍全池穷举？search 默认假设变了吗？**

## 5. 不必通读

- `trade/interpreter.rs` 全文（~1100 行，按 Phase 局部改；见 INTERNAL 地图）
- `manufacture/interpreter.rs` 全文（按 Phase 局部改；**勿按贸易站相位假设改**）
- `infra-cli/output.rs` 全文（按 `emit_*` 函数名定位子命令输出）
- `MECHANICS_REGISTRY.csv`、PRTS HTML 快照
- xlsx 练度表二进制

## 6. 验证

### 6.1 机制回归（改 skill / interpreter / shortcut 后）

```bash
python scripts/build_skill_table.py    # pilot 干员硬失败
cargo test -p infra-core
cargo run -p infra-cli -- verify --all
```

### 6.2 Agent 默认端到端探测（243 + 全精2）

**除非用户明确给了其他布局或 operbox，后续 Agent 一律用仓库标准夹具跑 `layout test`：**

| 用途 | 路径 |
|------|------|
| 243 布局（`BaseBlueprint`） | `data/fixtures/243/layout.json` |
| 全精2 练度盒（`OperBox`） | `data/fixtures/243/operbox_full_e2.json` |

```bash
cargo run -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text
```

说明：

- **不要**用 `bench` 代替（布局锁死、不含用户蓝图）。
- **不要**默认 `operbox_gongsun.json` 或用户 xlsx 导出——那是较小/个人练度；标准集成探测用 **`operbox_full_e2.json`**。
- 用户提供了自己的 `--layout` / `--operbox` 时，以用户路径为准；否则固定上述两文件。
- 代码侧等价路径：`default_operbox_full_e2_path()`；布局同 `BaseBlueprint::template_243_use_this()`。
- 夹具说明见 [data/fixtures/243/README.md](data/fixtures/243/README.md)。

## 7. 数据不变式

1. `skill_table.id` 必须等于解包 `buff_id`
2. 干员归属只在 `operator_instances.json`（`resolve_buff_ids` 处理 stepwise）
3. 贸易站技能原文只信 `prts_trade_skills.json`
4. `REGRESSION_CASES.csv` 的 `operators` 列**未**驱动夹具；按 `expect_shortcut` / `case_id` 映射（见 PROJECT_MAP）

## 8. 非目标（本仓库不做）

心情排班、宿管恢复、全基建连班优化 — 见 `EFFECT_ATOM_DESIGN.md` §8.12。上层规划器消费本求解器效率输出后再排班。
