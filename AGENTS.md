# Agent 引导（收尾 / bug 修复期首读）

> 本项目已接近收尾。默认工作模式从“继续实现架构计划”切换为“复现 bug、定位边界、最小修复、补回归、保持口径稳定”。

## 0. 当前状态

这是一个明日方舟基建效率 / 编排 / 排班引擎：

- `infra-core`：机制解释、搜索、编排、排班、导出数据结构。
- `infra-cli`：命令入口、文件加载、输出格式化、回归验证壳。
- `data/`：技能、干员实例、体系、shortcut、标准夹具等运行时真源。
- `docs/`：当前事实、收尾期流程、任务路由和历史归档。

现阶段的默认目标是稳定，不是扩张。除非用户明确要求新增功能，否则不要主动推进 `docs/TODO/` 里的历史 Phase 计划。

非目标仍然是：心情排班、宿管恢复、全基建连续时间最优化。

## 1. 首读顺序

1. 本文。
2. [docs/MAINTENANCE_MODE.md](docs/MAINTENANCE_MODE.md)：收尾期 bug 修复流程、分层定位、验收矩阵。
3. [docs/INDEX.md](docs/INDEX.md)：文档入口和任务路由。
4. [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)：当前代码地图、模块边界、数据真源。
5. 按 bug 类型读取对应领域文档；不要全仓库通读 Markdown。

`plans/` 和 `docs/TODO/` 默认是历史建设期材料。只有用户明确要求继续某个 TODO，或 bug 定位需要理解当时设计，才读取它们。

## 2. 收尾期默认动作

用户报告 bug / “结果不对” / “跑一下看看”时，按以下顺序：

1. 记录输入：命令、layout、operbox、assignment、期望值、实际值。
2. 先复现：优先用现有 CLI 入口，不在代码里临时拼新路径。
3. 缩小层级：CLI 参数/输出 → layout 编排 → search 排序 → 单站 solver → L1/L2/L3 机制 → data。
4. 最小修复：只改 bug 所在层；不顺手重构、不重开 Phase 计划。
5. 加回归：能落 CSV/fixture 就落 CSV/fixture；排班 bug 优先保留最小 layout + operbox 或 debug bundle。
6. 验证：跑与改动半径匹配的命令，说明未跑的原因。
7. Git：只提交本轮 agent 自己改的文件。

如果无法复现，不要猜公式；先给出已跑命令、输入差异和下一步需要的最小材料。

## 3. 硬边界

| 层 | 规则 |
|----|------|
| 数据 | `skill_table.id` 必须等于解包 `buff_id`；干员归属只在 `operator_instances.json` |
| L1 | `trade/interpreter.rs`、`manufacture/interpreter.rs` 只认 `buff_id`，不认识干员名 |
| L2 | `gold_flow.rs`、`order_mechanic.rs`、`unit_output.rs` 处理机制域最优解；`atoms: []` 可表示委托 |
| L3 | `shortcut.rs` + `trade_shortcuts.json` 处理固定最优 / 难 atom 化组合 |
| GL | `cross_facility/`、`global_resource/`、`control/` 处理跨设施资源与注入 |
| Layout | `build_plan -> execute_plan -> pipeline -> resolve_base` 是主路径；不要绕过它修排班 |
| Scoring | 跨贸易 / 制造 / power / global 注入默认拆分展示；需要排序必须经命名 policy |
| CLI | 不写机制、公式、求解；只做命令、加载、输出、回归 |

不要为了“零 warning”破坏 API / serde / 预留机制。当前允许保留 `private_interfaces`、未来机制 `dead_code`、预留字段 warning。

## 4. Bug 路由

| 现象 | 先读 | 优先入口 |
|------|------|----------|
| `plan` / MAA / 三队轮换不对 | [docs/MAINTENANCE_MODE.md](docs/MAINTENANCE_MODE.md)、[docs/SCHEDULE_ROTATION.md](docs/SCHEDULE_ROTATION.md)、[docs/INFRA_CLI.md](docs/INFRA_CLI.md) | `cargo run -q -p infra-cli -- plan ...` |
| 单班布局结果不对 | [docs/BASE_ASSIGNMENT.md](docs/BASE_ASSIGNMENT.md)、[docs/ADR/0001-layout-assignment-decomposition.md](docs/ADR/0001-layout-assignment-decomposition.md) | `layout test` / `layout eval` |
| 贸易站效率 / shortcut 不对 | [docs/EFFECT_ATOM_DESIGN.md](docs/EFFECT_ATOM_DESIGN.md)、[docs/INTERNAL/TRADE_INTERPRETER.md](docs/INTERNAL/TRADE_INTERPRETER.md)、[docs/INTERNAL/SHORTCUT_MATCHING.md](docs/INTERNAL/SHORTCUT_MATCHING.md) | `verify --case` / `trade yield` |
| 制造站效率不对 | [docs/MANUFACTURE_STATUS.md](docs/MANUFACTURE_STATUS.md) | `layout test` / `bench --recipe` |
| 中枢 / 全局注入 / 木天蓼不对 | [docs/INTERNAL/CROSS_FACILITY.md](docs/INTERNAL/CROSS_FACILITY.md)、[docs/SCORING_MODEL.md](docs/SCORING_MODEL.md) | `layout eval` / targeted core test |
| CLI / 前端调用问题 | [docs/INFRA_CLI.md](docs/INFRA_CLI.md)、[docs/FRONTEND_CLI.md](docs/FRONTEND_CLI.md)、[docs/FRONTEND_SERVE_GUIDE.md](docs/FRONTEND_SERVE_GUIDE.md) | 复用用户命令 |
| 数据缺漏 / 干员建模 | [docs/MODELLED_OPERATORS.md](docs/MODELLED_OPERATORS.md)、[docs/需要完成的干员建模.md](docs/需要完成的干员建模.md) | pool / verify / targeted fixture |

## 5. 改动策略

### 5.1 机制 bug

1. 先确认数据是否已有：`operator_instances.json`、`skill_table.json`、必要时 `trade_shortcuts.json` / `base_systems.json`。
2. 能用现有 `EffectAtom` 表达时只改数据。
3. 新 Selector / Action / Condition / Phase 才改 `types.rs`。
4. L1 真错才改 interpreter；贸易和制造不要互套假设。
5. 订单分布、赤金闭环、单位产出进 L2；固定最优组合进 L3 shortcut。
6. 增加或更新回归锚点。

### 5.2 排班 / 编排 bug

1. 先用 `plan` 或 `layout team-rotation` 复现用户路径。
2. 单班问题用 `layout test`；指定编制结算用 `layout eval`。
3. 只在以下层级中修对应问题：
   - 体系选型：`layout/orchestrate/select.rs`、`layout/system.rs`、`data/base_systems.json`
   - 落位语义：`layout/orchestrate/execute.rs`、`layout/assign/pipeline.rs`
   - 设施补位：`layout/assign/*_fill.rs`
   - 三队轮换：`schedule/team_rotation.rs`、`schedule/shift_bind.rs`
   - MAA 导出：`export/maa.rs`
4. 不为单一 bug 泛化 ADR 决策 D 的 execute_plan 三态；只有出现第二个数据驱动、且需要 anchor + 搜索半固定的体系时再抽象。

### 5.3 评分 / 输出 bug

1. 先判断是核心结果错，还是 CLI 展示错。
2. 核心分量错改 `infra-core`；列名、文本、JSON 形状错改 `infra-cli/output.rs`。
3. 不新增匿名综合权重。
4. 现有排序口径：
   - 贸易搜索：`score == trade_pct == order_eff_total`
   - 制造搜索：`prod_total`
   - 发电搜索：`charge_speed_pct`
   - 中枢搜索：`ControlInjectRawSumV0`，即 `trade_inject + manu_gold + manu_br` 的局部 heuristic

## 6. 默认命令

本仓库 warning 多。跑 Cargo 时先编译落日志，只看 tail；编译通过后再运行测试 / CLI。

```bash
mkdir -p target/codex-logs

cargo test -p infra-core --no-run > target/codex-logs/infra-core-test-build.log 2>&1
tail -80 target/codex-logs/infra-core-test-build.log
cargo test -p infra-core --quiet

cargo build -p infra-cli > target/codex-logs/infra-cli-build.log 2>&1
tail -80 target/codex-logs/infra-cli-build.log
cargo run -q -p infra-cli -- verify --all
```

编译失败时：

```bash
rg -n "error\\[|error:|failed|panicked" target/codex-logs
```

### 用户说“跑一遍模拟”

默认理解为：全精2 练度盒 + 243 布局 + 账号分析 + αβγ ABC 三队轮换 + 写出 MAA JSON。

```bash
cargo run -q -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

仅排班时：

```bash
cargo run -q -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

不要用 `layout rotation`（A-B-A，已废弃）或 `layout test`（单班探测）代替模拟。

### 改机制后的 smoke test

```bash
cargo run -q -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text
```

## 7. Git 协作默认

用户希望 agent 主动整理 Git，而不是只把改动留在工作区。除非用户明确说“不要动 git / 不要提交”：

1. 开始和结束都看 `git status --short`。
2. 只 stage / commit 本轮 agent 自己改的文件；不要用 `git add .`。
3. 若本轮改动和既有用户改动在同一文件内交织，不能可靠拆分时不要强行提交。
4. 验证通过，或验证未跑但原因已说明时，若本轮改动形成独立单元，默认创建简短 commit。
5. 不自动 `amend`、`rebase`、`reset`、`checkout --` 或清理未跟踪文件。

## 8. 不必通读

- `target/`、`out/`、release 产物、xlsx 二进制、`.venv/`。
- PRTS HTML 快照、`MECHANICS_REGISTRY.csv`。
- `plans/` 与 `docs/TODO/` 历史计划，除非用户明确要求继续对应事项。
- `docs/公孙长乐的体系分析文档/` 全部理论链，除非当前 bug 是体系 / 等效效率锚点。
- `trade/interpreter.rs`、`manufacture/interpreter.rs`、`infra-cli/output.rs` 全文；优先按内部地图或函数名定位。
