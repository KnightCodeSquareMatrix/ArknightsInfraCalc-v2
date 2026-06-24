# Agent 引导（新会话首读）

> 本文件是 agent runbook，只放当前规则、路由和默认命令。项目地图、长表和历史说明放在 `docs/`。

## 0. 项目边界

这是一个明日方舟基建效率 / 编排 / 排班引擎：

- `infra-core`：机制、搜索、编排、排班、导出所需数据结构。
- `infra-cli`：命令入口、文件加载、输出格式化、回归验证壳。
- `data/`：技能、干员实例、体系、shortcut、标准夹具等运行时真源。
- 当前架构目标：站内搜索使用清晰效率量纲；贸易赤金订单效率、制造赤金效率、制造经验效率等跨域结果按分量展示；需要局部排序时进入 `scoring` 命名 policy，不发明匿名综合分。

非目标：心情排班、宿管恢复、全基建连续时间最优化。

## 1. 首读顺序

1. 本文。
2. [docs/INDEX.md](docs/INDEX.md)：文档入口、TODO / ARCHIVE 分层。
3. [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)：当前架构、模块索引、数据真源。
4. 按任务读对应文档，避免全仓库通读 Markdown。

准备实现事项看 [docs/TODO/](docs/TODO/)。完成或废弃的事项归档到 [docs/ARCHIVE/](docs/ARCHIVE/)。

`plans/` 默认是历史设计记录；除非当前文档明确引用，否则不是首读材料。

## 2. 当前主线

评分 Phase 3 已从“平衡公式入口”收敛为分量化 policy 入口：

- 公孙长乐确认不需要贸易-制造平衡公式；龙舌兰、可露希尔、但书等已折算等效效率，直接进入贸易站赤金订单总效率。
- `crates/infra-core/src/scoring/` 已提供 `ScoringPolicyId`、`TradeManuEfficiencyComponents`、`ComponentScore`。
- `search/control.rs` 通过 `ControlInjectRawSumV0` policy 维持旧 `trade_inject + manu_gold + manu_br` 局部排序行为；这是 heuristic，不是理论公式。
- 后续主线是 [docs/SCORING_REFACTOR_PLAN.md](docs/SCORING_REFACTOR_PLAN.md) Phase 5：CLI / JSON 输出继续拆清 trade_gold_order / manu_gold / manu_br 等分量，并补公孙等效效率锚点。

用户说“继续下一步”“看准备实现的功能”时，先看 [docs/TODO/README.md](docs/TODO/README.md)；若无 ready TODO，不要自行发明综合权重。

## 3. 硬规则

| 层 | 约束 |
|----|------|
| L1 | `trade/interpreter.rs`、`manufacture/interpreter.rs` 只认 `buff_id`，不认识干员名 |
| L2 | `gold_flow.rs`、`order_mechanic.rs`、`unit_output.rs` 处理机制域最优解；`atoms: []` 可表示委托，不等于缺失 |
| L3 | `shortcut.rs` + `trade_shortcuts.json` 处理固定最优 / 难 atom 化组合 |
| GL | `cross_facility/`、`global_resource/`、`control/` 处理跨设施资源与注入 |
| Scoring | 同设施内可按设施效率排序；跨贸易 / 制造 / 全局注入默认拆分量展示；需要排序必须经命名 policy |
| CLI | 不写机制、公式、求解；只做命令、加载、输出、回归 |

不要为了“零 warning”破坏 API / serde / 预留机制。当前允许保留 `private_interfaces`、未来机制 `dead_code`、预留字段 warning。

## 4. 任务路由

| 任务 | 先读 |
|------|------|
| 评分 / 排序口径 | [docs/SCORING_MODEL.md](docs/SCORING_MODEL.md)、[docs/SCORING_REFACTOR_PLAN.md](docs/SCORING_REFACTOR_PLAN.md)、[docs/ARCHIVE/done/SCORING_PHASE3.md](docs/ARCHIVE/done/SCORING_PHASE3.md) |
| 编排 / 体系 / meta 组合 | [docs/ORCHESTRATION_LAYER.md](docs/ORCHESTRATION_LAYER.md)、[docs/BASE_ASSIGNMENT.md](docs/BASE_ASSIGNMENT.md) |
| CLI / 前端调用 | [docs/INFRA_CLI.md](docs/INFRA_CLI.md)、[docs/FRONTEND_CLI.md](docs/FRONTEND_CLI.md) |
| 贸易 L1/L2/L3 | [docs/EFFECT_ATOM_DESIGN.md](docs/EFFECT_ATOM_DESIGN.md)、[docs/INTERNAL/TRADE_INTERPRETER.md](docs/INTERNAL/TRADE_INTERPRETER.md)、[docs/INTERNAL/SHORTCUT_MATCHING.md](docs/INTERNAL/SHORTCUT_MATCHING.md) |
| 制造站 | [docs/MANUFACTURE_STATUS.md](docs/MANUFACTURE_STATUS.md) |
| 跨设施 global atom | [docs/INTERNAL/CROSS_FACILITY.md](docs/INTERNAL/CROSS_FACILITY.md) |
| 排班轮换 | [docs/SCHEDULE_ROTATION.md](docs/SCHEDULE_ROTATION.md) |
| 待建模 / 已建模干员 | [docs/需要完成的干员建模.md](docs/需要完成的干员建模.md)、[docs/MODELLED_OPERATORS.md](docs/MODELLED_OPERATORS.md) |

## 5. 改机制工序

改干员 / 技能 / 机制时按层走：

1. 数据层：`data/operator_instances.json`、`data/skill_table.json`、必要时 `data/trade_shortcuts.json` / `data/base_systems.json`。
2. 类型层：新 Selector / Action / Condition / Phase 先改 `types.rs`。
3. L1：贸易改 `trade/interpreter.rs`，制造改 `manufacture/interpreter.rs`，不要互套假设。
4. L2：订单分布、赤金闭环、单位产出进 `trade/order_mechanic.rs`、`gold_flow.rs`、`unit_output.rs`。
5. L3 / 编排：固定最优组合进 shortcut / `base_systems.json`，不要靠 `search` 临时发现 meta。
6. 验证：加或改测试，再按 §6 运行。
7. 文档：更新对应领域文档；准备实现事项放 `docs/TODO/`，完成后移入 `docs/ARCHIVE/done/`。

加干员前先判断：数据有了吗？L1 够吗？要不要 L2/L3？制造是否仍全池穷举？有没有混量纲排序？

## 6. 验证与默认命令

### 6.1 Cargo 输出纪律

本仓库 warning 多。跑 Cargo 时先编译落日志，只看 tail；编译通过后再运行测试 / CLI。

```powershell
New-Item -ItemType Directory -Force target/codex-logs | Out-Null

cargo test -p infra-core --no-run *> target/codex-logs/infra-core-test-build.log
Get-Content target/codex-logs/infra-core-test-build.log -Tail 80
cargo test -p infra-core --quiet

cargo build -p infra-cli *> target/codex-logs/infra-cli-build.log
Get-Content target/codex-logs/infra-cli-build.log -Tail 80
cargo run -q -p infra-cli -- verify --all
```

编译失败时先用：

```powershell
rg -n "error\[|error:|failed|panicked" target/codex-logs
```

### 6.2 用户说“跑一遍模拟”

默认理解为：全精2 练度盒 + 243 布局 + 账号分析 + αβγ ABC 三队轮换 + 写出 MAA JSON。默认入口用带分析的 `plan`；只有用户明确说“仅排班 / 不要分析”时，才用 `layout team-rotation`。

```powershell
cargo run -q -p infra-cli -- plan `
  --operbox data/fixtures/243/operbox_full_e2.json `
  --maa-out out/243_maa.json
```

仅排班时：

```powershell
cargo run -q -p infra-cli -- layout team-rotation `
  --layout data/fixtures/243/layout.json `
  --operbox data/fixtures/243/operbox_full_e2.json `
  --maa-out out/243_maa.json
```

不要用 `layout rotation`（A-B-A，已废弃）或 `layout test`（单班探测）代替模拟。

### 6.3 改机制后的 smoke test

```powershell
cargo run -q -p infra-cli -- layout test `
  --layout data/fixtures/243/layout.json `
  --operbox data/fixtures/243/operbox_full_e2.json `
  --text
```

### 6.4 Git 协作默认

用户希望 agent 主动整理 Git，而不是只把改动留在工作区。除非用户明确说“不要动 git / 不要提交”，完成代码或文档改动后按以下规则处理：

1. 开始和结束都看 `git status --short`，确认工作区是否已有用户改动。
2. 只 stage / commit 本轮 agent 自己改的文件；不要用 `git add .` 混入无关文件。
3. 若本轮改动和既有用户改动在同一文件内交织，不能可靠拆分时不要强行提交；在最终回复说明哪些文件需要用户确认或手动拆分。
4. 验证通过，或验证未跑但原因已说明时，若本轮改动形成独立单元，默认创建一个简短 commit。
5. 不自动 `amend`、`rebase`、`reset`、`checkout --` 或清理未跟踪文件；这些操作必须等用户明确要求。

## 7. 数据与评分不变式

1. `skill_table.id` 必须等于解包 `buff_id`。
2. 干员归属只在 `operator_instances.json`。
3. 贸易站技能原文只信 `prts_trade_skills.json`。
4. `REGRESSION_CASES.csv` 的 `operators` 列未驱动夹具；按 `expect_shortcut` / `case_id` 映射。
5. `skill_table.atoms = []` 可表示“交给 L2 / L3 / 特例域处理”。
6. 贸易搜索 `score == trade_pct == order_eff_total`；`gold_pct` 单独展示。
7. 制造搜索当前按 `prod_total`；发电搜索当前按 `charge_speed_pct`。
8. 中枢已通过 `ControlInjectRawSumV0` policy 维持 `trade_inject + manu_gold + manu_br` 旧排序行为；这是局部排序策略，不是平衡公式。
9. 跨贸易 / 制造 / power / global 注入比较时优先拆分量展示；若必须排序，只能新增命名 policy，不猜匿名权重。

## 8. 不必通读

- `target/`、`out/`、release 产物、xlsx 二进制、`.venv/`。
- PRTS HTML 快照、`MECHANICS_REGISTRY.csv`。
- `docs/公孙长乐的体系分析文档/` 全部理论链，除非当前任务是体系 / 等效效率锚点。
- `trade/interpreter.rs`、`manufacture/interpreter.rs`、`infra-cli/output.rs` 全文；按 `docs/INTERNAL/` 或函数名定位。
