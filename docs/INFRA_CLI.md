# infra-cli 模块职责

> **定位**：`infra-cli` 是薄命令行外壳——解析参数、加载 `data/`，调用 `infra-core` 求解，再把结果格式化为 CSV / 文本 / JSON。**不在此 crate 实现游戏机制或效率公式**；机制真相在 `infra-core`，数据真相在 `data/`。

协作总览仍见 [PROJECT_MAP.md](PROJECT_MAP.md)；机制设计见 [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md)。

---

## 分层原则

```
argv + data/ 文件
      ↓
infra-cli   参数解析 · 数据路径 · 输出格式 · 回归夹具
      ↓
infra-core  pool / search / schedule / solve_trade_*
      ↓
结果 → infra-cli output 层写 stdout/stderr/文件
```

| 层 | 允许做 | 禁止做 |
|----|--------|--------|
| **main / commands** | 子命令分发；把 argv 转成 core 的输入结构；组合多次 core 调用 | 解释 EffectAtom；手写 trade%/gold% 公式 |
| **verify** | 回归用例加载；硬编码 `TradeRoomInput` 夹具；PASS/FAIL 断言与打印 | 修改求解逻辑（应改 core 或 CSV 期望值） |
| **output** | `OutputOptions`、CSV BOM、列名、人类可读标签 | 调用 `solve_*` 或改变评分 |

**依赖方向**：`commands` → `verify` / `output` → `infra-core`。`verify` 与 `output` 互不依赖。

---

## 目录与职责（当前）

```
crates/infra-cli/src/
├── main.rs              # 进程入口 + 子命令路由（其余子命令暂留此处，见「待拆」）
├── commands/
│   ├── mod.rs           # 子命令模块聚合；对外 re-export
│   ├── plan.rs          # `plan`：box profile + αβγ 排班 + MAA
│   ├── layout.rs        # `layout test` / `rotation` / `team-rotation` / `analyze` / `eval` 全部子命令
│   ├── profile.rs       # `profile`：性能画像 / 分析链路对比辅助
│   └── verify.rs        # `verify` 子命令：跑回归、汇总失败
├── verify/
│   ├── mod.rs           # 回归资产门面；re-export loaders 与 fixtures
│   ├── cases.rs         # 读 CSV → `RegressionCase` / `UnitAnchorCase`
│   └── fixtures.rs      # 硬编码 `TradeRoomInput`（verify + trade yield 共用）
└── output.rs            # 各子命令的 emit_* 与 CSV/文本/JSON 写入
```

### `main.rs`

| 职责 | 说明 |
|------|------|
| `main` / `run` | `ExitCode` 包装；按 `args[1]` 分发子命令 |
| `print_usage` | 用法说明（stderr） |
| **暂留** | `pool` / `search` / `schedule` / `trade` / `bench` 及共享参数解析（`--roster`、`--operbox` 等） |

改子命令路由或全局 usage 时改这里；**不要**把新的回归夹具或 CSV 结构塞回 `main.rs`。

### `commands/`

每个文件对应**一个用户可见子命令**的编排逻辑：读参数 → 调 `infra-core` → 调 `output::emit_*`。

| 模块 | 职责 | 不负责 |
|------|------|--------|
| `plan.rs` | **`plan`**：box profile JSON + `schedule_team_rotation` + MAA；`--operbox` 支持 JSON/xlsx；布局默认 243 | 画像算法（`box_profile/`）；排班逻辑（`schedule/`） |
| `layout.rs` | `layout test` / `rotation` / `team-rotation` / `analyze` / `eval`：蓝图 JSON + operbox → `assign_shift` 宏观落位（或自定义 `--assignment`）→ `resolve_base` → 搜索/评分 | 蓝图格式定义（`infra-core::layout::blueprint`）；求解公式（在 `infra-core`） |
| `profile.rs` | `profile layout-full` / `profile analyze-compare`：采集 CLI 热路径、搜索规模和分析链路耗时 | 业务求解公式；用户主流程输出契约 |
| `verify.rs` | `verify_cmd`：遍历 `REGRESSION_CASES.csv`；按 `expect_shortcut` 选夹具；对比 trade%/gold%/shortcut；再跑 `UNIT_OUTPUT_ANCHORS.csv` | 夹具定义（在 `verify/fixtures.rs`）；CSV 列定义（在 `verify/cases.rs`） |

项目结构已定型：`pool` / `search` 等编排暂留 `main.rs`，**不再计划拆文件**。新增子命令仍应优先新建 `commands/foo.rs`，避免继续膨胀 `main.rs`。

### `verify/`

回归与探测用的**测试资产**，与「用户命令」分离，避免 `main.rs` 膨胀。

#### `cases.rs` — 期望值与元数据（来自 CSV）

- 解析 `data/REGRESSION_CASES.csv`、`data/UNIT_OUTPUT_ANCHORS.csv`
- 只定义结构体与 `load_*`；**不含**干员 buff 组合

改 CSV 列布局时只改此文件（及 `data/` 里对应 CSV）。

#### `fixtures.rs` — 输入房间（硬编码干员）

| 函数 | 用途 |
|------|------|
| `closure_fixture` | 可露希尔分档回归（`reg_gsl_closure_tier*`） |
| `witch_fixture` | 巫恋核 shortcut 回归（`gsl_witch_*`） |
| `docus_fixture` | 但书 solo（`gsl_docus_*`） |
| `unit_fixture` | 单位产出锚点 + `trade yield <fixture>` 探测 |

**重要**：`REGRESSION_CASES.csv` 的 `operators` 列目前**未**驱动夹具选择；`verify` 按 `expect_shortcut` / `case_id` 映射到上述函数。扩展新回归族时：**夹具加在 `fixtures.rs`，断言逻辑加在 `commands/verify.rs`，期望值加在 CSV**。

#### `commands/verify.rs` — 断言编排

- 决定跑哪些 case（`--case` / `--all`）
- 跳过尚未接线的 case（`fixture not wired`）
- 调用 `solve_trade_with_shift`，比较容差，打印 PASS/FAIL
- 任一失败 → `Error::msg("regression failures")`

### `output.rs` — 呈现层

| 导出 | 对应命令 |
|------|----------|
| `OutputOptions` / `from_args` | 全局 `--text` / `--json` / `-o` |
| `emit_pool` | `pool` |
| `emit_trade_search` | `search trade` |
| `emit_bench` | `bench` |
| `emit_schedule` | `schedule rotation` |
| `emit_trade_yield` | `trade yield` |

约定：默认 **CSV**（写文件时 UTF-8 BOM）；`--text` 走 stderr 人类可读。新增子命令时先定 `emit_*` API，再在 `commands/*.rs` 里调用。

---

## 子命令 → 模块对照

| 用户命令 | 编排（当前） | 输出 | 数据 / 夹具 |
|----------|--------------|------|-------------|
| **`plan`** | `commands/plan.rs` | profile JSON 文件 + stdout 分析/排班表；可选 MAA | 必选 `--operbox`（JSON/xlsx）；布局默认 `data/fixtures/243/layout.json` |
| `verify` | `commands/verify.rs` | stdout/stderr 行文本 | `verify/cases.rs` + `verify/fixtures.rs` + `data/*.csv` |
| `pool` | `main.rs` | `output::emit_pool` | operbox / roster → `infra-core::pool` |
| `search trade` | `main.rs` | `output::emit_trade_search` | roster / operbox |
| `bench` | `main.rs` | `output::emit_bench` | 必选 `--operbox`；布局固定 `search_baseline`（`243_use_this_.json`） |
| **`layout test`** | `commands/layout.rs` | `output::emit_bench` | 必选 `--layout` + `--operbox`；默认调用 `assign_base_greedy` |
| **`layout team-rotation`** | `commands/layout.rs` | `output::emit_team_rotation` | 必选 `--layout` + `--operbox`；**αβγ ABC 轮换**；仅排班时用 |
| **`layout rotation`** | `commands/layout.rs` | `output::emit_base_rotation` | **已废弃**（A-B-A）；启动时 stderr 警告，请改用 `team-rotation` |
| **`layout analyze`** | `commands/layout.rs` | `print_box_profile_report` | 必选 `--layout` + `--operbox`；练度概况分析 |
| **`layout eval`** | `commands/layout.rs` | stderr 文本 / JSON | 必选 `--layout` + `--operbox` + `--assignment`；评估指定编制 |
| `profile layout-full` | `commands/profile.rs` | stderr 性能报告 | 开发辅助；默认路径为历史性能夹具，用户模拟不要用 |
| `profile analyze-compare` | `commands/profile.rs` | stderr 对比报告 | 开发辅助；对比 hybrid profile 与旧 probe 链路耗时 |
| `schedule rotation` | `main.rs` | `output::emit_schedule` | 贸易站 A-B-A（旧入口，已废弃） |
| `trade yield` | `main.rs` | `output::emit_trade_yield` | `verify::unit_fixture` |

---

## 常见改动应改哪里

| 你想做的事 | 改哪里 | 不要改 |
|------------|--------|--------|
| 新增回归 case | `data/REGRESSION_CASES.csv`；必要时 `fixtures.rs` + `commands/verify.rs` 分支 | `interpreter.rs`（除非机制真错了） |
| 新 shortcut 族夹具 | `verify/fixtures.rs` | `main.rs` |
| 单位产出锚点 | `data/UNIT_OUTPUT_ANCHORS.csv` + `unit_fixture` 名 | `unit_output.rs`（除非公式错） |
| 新子命令 | 新建 `commands/foo.rs` + `output` emit + `main` 分发 | 在 `output` 里写求解 |
| CSV 列名/列序 | `verify/cases.rs` | 散落在多个命令里重复解析 |
| 表格列或中文标签 | `output.rs` | `infra-core` |
| 搜索/排班行为 | `infra-core` | `infra-cli`（最多改传参） |
| 自定义基建布局 + 练度盒探测 | `layout test`（见下节）；**不要**手写 `LayoutContext` / 搜索上下文或改 `bench` 硬编码 | 在 CLI 里复制搜索公式 |

---

## `plan`（账号分析 + 排班，推荐入口）

> **给 Cursor / 协作者**：用户要「分析练度 + 出排班 + MAA」时，优先用 **`plan`**。`layout team-rotation` 仅做排班（无 profile JSON、需显式 `--layout`）。前端集成见 [FRONTEND_CLI.md](FRONTEND_CLI.md)。

```bash
cargo run -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

| 参数 | 说明 |
|------|------|
| `--operbox` | **必填**。`OperBox` JSON 或一图流练度 xlsx |
| `--layout` | 可选。默认 `data/fixtures/243/layout.json` |
| `--maa-out` | 写出 MAA 基建排班 JSON |
| `--profile-out` | 可选。账号画像 JSON 路径（默认同目录 `*_profile.json`） |
| `--output-dir` | 可选。写出三队 `team_shift_*.json` assignment |
| `--baseline` | 可选。对比用基准 operbox（默认 `data/box_profile_knightcode.json`） |
| `--top` | Top-K 搜索条数，默认 20 |
| `--maa-title` | 覆盖 MAA JSON 顶层 `title` |
| `--json` | 仅输出 profile JSON 到 stdout（跳过人类可读表） |

---

## 跑一遍模拟（Agent 默认）

> **给 Cursor / 协作者**：用户说「跑一遍模拟」「跑模拟」「三班模拟」等，且未指定其他命令时，**默认**跑 **`plan`** 并**写出 MAA JSON**，因为它同时包含账号分析与 αβγ 排班。只有用户明确说“仅排班 / 不要分析”时，才用 `layout team-rotation`。不要用 `layout rotation`（A-B-A）或 `layout test`（单班搜索）代替。

**无用户指定路径时，Agent 固定用：**

| 项 | 路径 |
|----|------|
| 布局 | `data/fixtures/243/layout.json` |
| 练度盒 | `data/fixtures/243/operbox_full_e2.json`（243 三班干员全精2 / 90） |
| MAA 输出 | `out/243_maa.json` |

### 命令（Agent 默认）

```bash
# Agent 默认
cargo run -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json

# 仅排班
cargo run -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

| 参数 | 说明 |
|------|------|
| `--layout` | `plan` 可选，默认 `data/fixtures/243/layout.json`；`layout team-rotation` 必填 |
| `--operbox` | **必填**。`OperBox` JSON；**Agent 默认** `data/fixtures/243/operbox_full_e2.json` |
| `--maa-out` | **Agent 默认必带**。写出 MAA 基建排班 JSON；默认 `out/243_maa.json` |
| `--maa-title` | 可选。覆盖 JSON 顶层 `title` |
| `--top` | Top-K 搜索条数，默认 20 |
| `--text` | 可选。带 `--maa-out` 时 stderr 默认已有人类可读排班表 |

### 输出约定

| 流 | 说明 |
|----|------|
| **stderr** | 三队花名册、轮换表、各班设施上岗与加权产出 |
| **`--maa-out` 文件** | MAA 协议 JSON（见 [FRONTEND_CLI.md](FRONTEND_CLI.md) §6） |

用户指定 `--layout` / `--operbox` / `--maa-out` 时以用户为准。MAA 映射实现见 `crates/infra-core/src/export/maa.rs`。

---

## 自定义布局 + 练度盒测试（改机制 smoke test）

> **给 Cursor / 协作者**：用户给出「某布局 JSON + operbox / 练度表」要跑**单班贸易/制造搜索探测**时，**优先用 `layout test`**，不要用 `bench`（`bench` 布局锁死 243c 基准）、也不要在 CLI 里临时拼 `LayoutContext` / 搜索上下文。若用户只说「跑一遍模拟」，见上节 **`plan`**。
>
> **无用户指定文件时，Agent 默认固定用：**
> - 布局：`data/fixtures/243/layout.json`
> - 练度盒：`data/fixtures/243/operbox_full_e2.json`（243 三班干员全精2 / 90）

### 何时用

| 场景 | 用哪个 |
|------|--------|
| **用户说「跑一遍模拟」（无用户路径）** | **`plan`** + 标准夹具 + **`--maa-out out/243_maa.json`**（见上节） |
| **改机制后 smoke test（无用户路径）** | **`layout test`** + **`data/fixtures/243/layout.json`** + **`data/fixtures/243/operbox_full_e2.json`** |
| 用户提供了 `BaseBlueprint` JSON（如 `243测试用布局.json`、排班工具导出的布局） | **`layout test`** + 用户 `--layout` + 用户或标准 `--operbox` |
| 对比固定 243c 基准 + operbox（无自定义房间结构） | `bench --operbox data/fixtures/243/operbox_full_e2.json` |
| 怪猎账号（木天蓼 12、泰拉调查团、精2 全局 +7/+2） | 代码侧 `LayoutContext::snhunt_baseline()` / `LayoutContext::snhunt_elite2_baseline()`，或 `resolve_snhunt_*_layout()` 生成布局快照；CLI 侧优先用蓝图 + assignment 含中枢双人 |
| 机制回归、shortcut 断言 | `verify --case …` / `verify --all` |
| 单站硬编码三人组产量 | `trade yield <fixture>` |

### 命令（Agent 默认）

```bash
cargo run -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  [--top <n>] \
  [-o <file.csv>] \
  [--text]
```

用户指定路径时，将 `--layout` / `--operbox` 换成用户文件即可：

```bash
cargo run -p infra-cli -- layout test \
  --layout <蓝图.json> \
  --operbox <练度盒.json> \
  [--top <n>] \
  [-o <file.csv>] \
  [--text]
```

| 参数 | 说明 |
|------|------|
| `--layout` | **必填**。任意路径的 `BaseBlueprint` JSON；**Agent 默认** `data/fixtures/243/layout.json` |
| `--operbox` | **必填**。玩家练度盒 JSON（`OperBox`）；**Agent 默认** `data/fixtures/243/operbox_full_e2.json`（全精2）；用户自有练度或 `data/operbox_gongsun.json` 仅在用户指定时用 |
| `--top` | Top-K 条数，默认 3 |
| `-o` / `--output` | 写 CSV（UTF-8 BOM）；缺省 stdout |
| `--text` | 人类可读摘要写 stderr（**Agent 本地探测时推荐**） |

### 内部链路（`layout test`）

```
BaseBlueprint::load(--layout)
  + OperBox::load(--operbox)
  + operator_instances.json + skill_table.json
        ↓
assign_base_greedy()  →  BaseAssignment（全基建宏观落位；可选 --assignment 覆盖）
        ↓
resolve_base()  →  LayoutContext（宿舍/发电/全局资源/贸易站数等）
        ↓
blueprint.trade_station_scenario()  →  TradeSearchOrderMode
blueprint.manu_line_scenario()      →  ManuSearchRecipeMode::Lines
        ↓
build_trade_pool / build_manufacture_pool → search_*_triples
        ↓
emit_bench（meta.layout = 蓝图路径）
```

### 布局 JSON 约定

- 结构与 `data/layout/243c.json` 一致：`rooms[]`（`kind` / `level` / `product`）、`scenario`（`dorm_occupant_count`、`sui_facility_count`、`initial_global` 等）、可选 `template` 元数据。
- 贸易订单分布、制造产线数**从 `rooms` 自动推导**，不必与 243c 相同（例如 2 贸易站 = 1 赤金 + 1 源石）。
- 宿舍推荐同时填写 `dorm_beds`（MAA 宿舍容量）和 `dorm_ambience_level`（“每间宿舍每级”技能读取值；满宿舍通常为 5）；旧布局未填写 `dorm_ambience_level` 时兼容读取 `dorm_beds`。
- `scenario` 在无进驻编制时作为布局聚合量回退（精英设施数、宿舍人数等）。
- 进驻编制（`BaseAssignment`）**可通过 `--assignment` 传入**（`layout test --assignment <path>` 或 `layout eval --assignment <path>`）；不传时**默认调用 `assign_base_greedy`** 自动生成全基建宏观落位。
- 怪猎木天蓼 / 精2 全局注入在 `infra-core` 用 `snhunt_default_assignment()`、`resolve_snhunt_*_layout()` 测；完整模拟需在蓝图 JSON 的 assignment 中编 `control`（**火龙S黑角** + **麒麟R夜刀**，≠ 三星黑角/夜刀）。
- **全基建宏观排班**（`assign_base_greedy` / `assign_shift` + 全局 `used` 落位）设计见 **[BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md)**；**已落地**，`layout test` 默认调用。

### 布局基准对照

| 基准 | 入口 | 木天蓼 | 全贸易 +7% | 全制造 +2% | 宿舍默认 |
|------|------|--------|------------|------------|----------|
| 公孙 243 事实布局 | `search_baseline()` / `bench` | 0 | 0 | 0 | 20 |
| 怪猎精0 | `snhunt_baseline()` | 12 | 0 | 0 | 20 |
| 怪猎精2 双人中枢 | `snhunt_elite2_baseline()` | 12 | 7 | 2 | 20 |

模板：`data/layout/snhunt.json`；编制见 `layout/resolve.rs` 的 `snhunt_control_assignment`。

### 示例（仓库内）

```bash
cargo run -p infra-cli -- layout test \
  --layout "243测试用布局.json" \
  --operbox data/operbox_gongsun.json \
  --text
```

### Agent 操作清单

1. 确认布局文件能通过 `BaseBlueprint::load`（缺字段对照 `data/layout/243c.json`）。
2. 确认 operbox 路径存在；用户未指定时可用 `data/operbox_gongsun.json` 或询问其练度表路径。
3. 运行 `layout test --text`，读 stderr 的贸易 split 线、制造 split 线与池统计。
4. 机制改动后：先 `cargo test -p infra-core`，再对**同一布局 + operbox** 重跑 `layout test` 做前后对比。
5. 不要把此流程换成 Python 脚本拼 layout，除非用户明确要求。

---

## 大文件导航（不拆分）

| 文件 | 按函数定位 |
|------|------------|
| `main.rs` | `pool_cmd` / `search_cmd` / `schedule_cmd` / `trade_cmd` / `bench_cmd` |
| `output.rs` | `emit_pool`、`emit_trade_search`、`emit_bench`、`emit_schedule`、`emit_trade_yield` |

新增输出先加 `emit_*`，再在对应 `*_cmd` 调用；保持「编排 vs 呈现」分离。

---

## 验证

```bash
cargo build -p infra-cli
cargo run -p infra-cli -- verify --all
cargo run -p infra-cli -- trade yield closure_solo --text
# 自定义布局 + 练度盒（见上节）
cargo run -p infra-cli -- layout test --layout 243测试用布局.json --operbox data/operbox_gongsun.json --text
```

回归是 CLI 层与 `data/` 的契约测试；**自定义基建场景**用 `layout test`；核心逻辑仍以 `cargo test -p infra-core` 为准。

---

## 相关文档

| 文档 | 内容 |
|------|------|
| [INDEX.md](INDEX.md) | 文档入口、TODO / ARCHIVE 分层、任务路由 |
| [PROJECT_MAP.md](PROJECT_MAP.md) | 全仓库地图、`infra-core` 索引、`data/` 职责 |
| [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) | 求解分层 L1/L2/L3 |
| [TODO/](TODO/) | 准备实现 / 正在实现的事项 |
