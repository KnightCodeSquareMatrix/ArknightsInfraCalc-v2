# infra-cli 模块职责

> 文档角色：canonical
> 生命周期状态：current
> 领域键：architecture.cli
> 当前真源：self
> 摘要：裁决 infra-cli 分层职责和命令边界

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
| **main / commands** | 子命令分发；把 argv 转成 core 的输入结构；组合多次 core 调用 | 解释 EffectAtom；手写生产效率公式 |
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
│   ├── advice.rs        # `advice`：legacy 练卡诊断；前端练卡由 `serve plan.compute` 随排班返回
│   ├── bake.rs          # `bake`：生成贸易/制造 3/2/1 人单房候选索引表（运行时验证后优先读取）
│   ├── debug.rs         # `debug rerun`：从生产 bundle 对当前运行时做可追溯重跑
│   ├── plan.rs          # `plan`：box profile + αβγ 排班 + MAA
│   ├── plan_compute.rs  # `plan` / `serve` 共用的一次 Plan 编排
│   ├── prefix_shadow.rs # 动态 producer V0/V1 同 Box shadow 差异 JSON
│   ├── serve.rs         # `serve`：前端常驻 worker；stdin/stdout JSON line 协议
│   ├── layout.rs        # `layout test` / `team-rotation` / `analyze` / `eval` 全部子命令
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
| **暂留** | `pool` / `search` / `trade` / `bench` 及共享参数解析（`--roster`、`--operbox` 等） |

改子命令路由或全局 usage 时改这里；**不要**把新的回归夹具或 CSV 结构塞回 `main.rs`。

### `commands/`

每个文件对应**一个用户可见子命令**的编排逻辑：读参数 → 调 `infra-core` → 调 `output::emit_*`。

| 模块 | 职责 | 不负责 |
|------|------|--------|
| `advice.rs` | **`advice`**：legacy 独立诊断。默认/`--explain`/`--answer` 消费旧 v2 规则；`--player` 可输出玩家 schema 用于本地诊断，但不属于前端协议 | 前端产品调用；前端必须从 `serve plan.compute.result.training_advice` 消费同一轮排班结果，完整 wire 合同见 [FRONTEND_CLI.md](FRONTEND_CLI.md) §4.5 |
| `bake.rs` | `bake`：并行生成 schema v12 的整数效率 `combo_table.bin`、`operators.json`、`manifest.json`；贸易行显式保存首批 room-local 机制签名，manifest 保存 table hash/count；`bake validate` 校验当前 CLI 指纹和完整贸易 row universe | 贸易/制造效率公式；手写搜索排序 |
| `debug.rs` | `debug rerun`：读取 legacy `method=plan` 生产 bundle 的内联快照，以当前 CLI / data 重跑正式 rotation，输出稳定 assignment 投影、原结果对照和运行时指纹 | 冒充线上 bit-identical replay；执行 bundle 中的路径/命令；修改求解或候选 |
| `plan_compute.rs` | `plan` 与 `serve` 共用的唯一 Plan 编排：一次 user rotation，再按调用方请求派生 profile/MAA | argv、文件写出、wire envelope、机制公式 |
| `plan.rs` | **`plan`**：解析 argv/文件，调用共享 Plan 编排，再写 profile/MAA 和人类输出 | 画像算法（`box_profile/`）；排班逻辑（`schedule/`） |
| `serve.rs` | `serve`：常驻读取 stdin JSON line；按发布的 JSON Schema 校验 `plan.compute` 内联参数，legacy `plan` 暂时保留路径适配 | 新业务公式；第二套 Plan 编排 |
| `layout.rs` | `layout test` / `team-rotation` / `analyze` / `eval`：蓝图 JSON + operbox → `assign_shift` 宏观落位（或自定义 `--assignment`）→ `resolve_base` → 搜索/效率结算；`eval --explain-json` 只投影 core settlement trace | 蓝图格式定义（`infra-core::layout::blueprint`）；求解公式或注入聚合解释（在 `infra-core`） |
| `profile.rs` | `profile layout-full` / `profile analyze-compare`：采集 CLI 热路径、搜索规模和分析链路耗时 | 业务求解公式；用户主流程输出契约 |
| `verify.rs` | `verify_cmd`：遍历 `REGRESSION_CASES.csv`；按 `rule_id` 选夹具；对比三位小数最终效率、机制等效效率与规则 ID；再跑 `UNIT_OUTPUT_ANCHORS.csv` | 夹具定义（在 `verify/fixtures.rs`）；CSV 列定义（在 `verify/cases.rs`） |

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

**重要**：`REGRESSION_CASES.csv` 的 `operators` 列目前**未**驱动夹具选择；`verify` 按 `rule_id` / `case_id` 映射到上述函数。扩展新回归族时：**夹具加在 `fixtures.rs`，断言逻辑加在 `commands/verify.rs`，期望值加在 CSV**。

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
| `emit_trade_yield` | `trade yield` |

约定：默认 **CSV**（写文件时 UTF-8 BOM）；`--text` 走 stderr 人类可读。新增子命令时先定 `emit_*` API，再在 `commands/*.rs` 里调用。

---

## 子命令 → 模块对照

| 用户命令 | 编排（当前） | 输出 | 数据 / 夹具 |
|----------|--------------|------|-------------|
| **`plan`** | `commands/plan.rs` -> `commands/plan_compute.rs` | profile JSON 文件 + stdout 分析/排班表；可选 MAA / prefix shadow JSON | 必选 `--operbox`（JSON/xlsx）；布局默认 `data/fixtures/243/layout.json` |
| `serve` | `commands/serve.rs` -> `commands/plan_compute.rs` | `plan.compute` 内联 profile/rotation/MAA；legacy `plan` 可写文件 | 常驻 worker；stdout 仅 JSON response line，stderr 仅日志 |
| `debug rerun` | `commands/debug.rs` -> `schedule_timed_rotation` | stdout 脱敏 hash/状态摘要；显式私有目录保存稳定行为投影、原结果比较和完整 rotation | 当前只支持成功的 `beta-test-bundle-v2-next-serve` legacy `method=plan` bundle；不消费线上绝对路径 |
| `bake` | `commands/bake.rs` | 本地 `data/baked` catalog + stderr progress/summary | `infra-core::bake`；生成后自动校验 signature/row，并抽样用 live solver 对账 response；`bake verify` 可对既有 catalog 重跑门禁 |
| `verify` | `commands/verify.rs` | stdout/stderr 行文本 | `verify/cases.rs` + `verify/fixtures.rs` + `data/*.csv` |
| `pool` | `main.rs` | `output::emit_pool` | operbox / roster → `infra-core::pool` |
| `search trade` | `main.rs` | `output::emit_trade_search` | roster / operbox |
| `bench` | `main.rs` | `output::emit_bench` | 必选 `--operbox`；布局固定 `search_baseline`（`243_use_this_.json`） |
| **`layout test`** | `commands/layout.rs` | `output::emit_bench` | 必选 `--layout` + `--operbox`；默认调用 `assign_base_greedy` |
| **`layout team-rotation`** | `commands/layout.rs` | `output::emit_team_rotation`；可选 prefix shadow JSON | 必选 `--layout` + `--operbox`；支持默认 ABC、ABC `12/12/12`、二班与两个具名特殊四班；仅排班时用 |
| **`layout analyze`** | `commands/layout.rs` | `print_box_profile_report` | 必选 `--layout` + `--operbox`；练度概况分析 |
| **`layout eval`** | `commands/layout.rs` | stderr 文本 / 默认汇总 JSON；`--explain-json` 输出 versioned 结算诊断 | 必选 `--layout` + `--operbox` + `--assignment`；评估指定编制 |
| `profile layout-full` | `commands/profile.rs` | stderr 性能报告 | 开发辅助；默认路径为历史性能夹具，用户模拟不要用 |
| `profile analyze-compare` | `commands/profile.rs` | stderr 对比报告 | 开发辅助；对比 hybrid profile 与旧 probe 链路耗时 |
| `profile bake-dependencies` | `commands/profile.rs` | JSON 依赖报告 | 只读扫描 skill table，穷举分类房内、同设施、跨设施、全局布局和运行时依赖；可选 `--layout <path>` 编译该蓝图的 L2 外部场景域；为条件化 Bake 规模设计提供输入 |
| `trade yield` | `main.rs` | `output::emit_trade_yield` | `verify::unit_fixture` |

开发和正式发布 catalog 时使用 `scripts/bake_and_verify.sh [--out <dir>]`。它构建 release CLI、
执行 `bake all` 的 catalog/live 抽样差分与机制门禁，再运行完整
`cargo test --release --workspace`；任一步失败都不会形成可发布结论。发布二进制自身无法携带
Rust `#[test]` harness，因此 CLI 内门禁与仓库完整测试门禁分层执行。

---

## 常见改动应改哪里

| 你想做的事 | 改哪里 | 不要改 |
|------------|--------|--------|
| 新增回归 case | `data/REGRESSION_CASES.csv`；必要时 `fixtures.rs` + `commands/verify.rs` 分支 | `interpreter.rs`（除非机制真错了） |
| 重跑生产 debug bundle | `debug rerun --bundle ...`；需要固定编制结算时再用写出的 `team_shift_*.json` 调 `layout eval` | 手工执行 bundle 的 `command` / `savedFiles` 路径 |
| 新 shortcut 族夹具 | `verify/fixtures.rs` | `main.rs` |
| 单位产出锚点 | `data/UNIT_OUTPUT_ANCHORS.csv` + `unit_fixture` 名 | `unit_output.rs`（除非公式错） |
| 新子命令 | 新建 `commands/foo.rs` + `output` emit + `main` 分发 | 在 `output` 里写求解 |
| CSV 列名/列序 | `verify/cases.rs` | 散落在多个命令里重复解析 |
| 表格列或中文标签 | `output.rs` | `infra-core` |
| 搜索/排班行为 | `infra-core` | `infra-cli`（最多改传参） |
| 自定义基建布局 + 练度盒探测 | `layout test`（见下节）；**不要**手写 `LayoutContext` / 搜索上下文或改 `bench` 硬编码 | 在 CLI 里复制搜索公式 |

---

## 生产反馈 current-runtime rerun

`debug rerun` 从生产 `debug-bundle.json` 读取内联 layout / operbox，并调用当前正式
`schedule_timed_rotation`。它用于判断“当前工作树对同一输入得到什么结果”，不是线上二进制的
bit-identical replay：旧 bundle 没有记录线上可执行文件与 runtime data 哈希，报告会把原运行时
明确标为 `unknown`。

```bash
cargo run -q -p infra-cli -- debug rerun \
  --bundle feedback/<date>/<case>/debug-bundle.json \
  --room trade_1 \
  --shift 1 \
  --output-dir target/codex-runs/<task>/private-rerun \
  --pretty
```

| 参数 | 说明 |
|---|---|
| `--bundle` | 必填；支持 `version=beta-test-bundle-v2-next-serve`、成功的 `serveRequest.method=plan` 或 `plan.compute` bundle |
| `--top` / `--rotation` | 可选显式覆盖；未提供时优先读取 recorded request，否则记录为 historical default |
| `--room` / `--operator` | 互斥的结果聚焦投影；只过滤结果、不改变求解输入；因包含私人编制，必须同时使用 `--output-dir` |
| `--shift` | 1-based；必须与 `--room` 或 `--operator` 一起使用 |
| `--output-dir` | 可选的新目录；拒绝覆盖，只写固定文件名的 layout、operbox、各班 assignment 和报告 |
| `--pretty` | pretty JSON；默认单行 JSON |

输入选择规则：bundle 内联快照为主；同目录存在 `layout.json` / `operbox.json` 时必须与内联
canonical hash 一致，缺失内联值时才回退 sibling。bundle 中的 `command`、`savedFiles`、request
路径和用户标签永不作为本地执行目标或输出路径。

私有报告中的 `comparison` 只比较稳定行为投影：rotation、三类 daily、班次时长/transition、
房间和有序干员名单；排除 elapsed、线上路径和标题。显式覆盖 `--top` 或 `--rotation` 后，比较
固定标记为 `not_comparable`。`current_runtime` 在重跑前对当前二进制和明确列出的运行时 catalog
计算 SHA-256；它不能补造旧线上环境。

stdout 只输出不含 source name、排班名单、focus 或 recorded projection 的脱敏摘要。完整 rotation、
focus、原结果投影和 operbox 只进入 `--output-dir`；该目录固定以私有 evidence 处理，不提交。
当前不支持失败调用重跑、全候选日志或“未选中候选为什么失败”的解释。`plan.compute` bundle 的
layout/operbox 以请求和 bundle 顶层的内联快照为准；同目录旧 sibling 快照不覆盖这两个请求快照。
历史 `plan.compute` bundle 可能把 `rarity=1` 作为未知星级哨兵，但仍保留真实 `elite/level`；重放只在
私有 debug 适配层放宽这项历史输入校验并记录数量，不会把 E1/E2 降级。正常运行时的 `plan.compute`
契约校验仍保持严格。

---

## 固定编制 settlement explain

`layout eval --explain-json` 对指定 assignment 运行与默认入口相同的 `resolve_base` 和各房正式
solver，但额外打开 call-scoped 中枢 L1 trace。默认 `layout eval` JSON / `--text` 契约不变；
`--text` 与 `--explain-json` 互斥。

```bash
cargo run -q -p infra-cli -- layout eval \
  --layout target/codex-runs/<task>/private-rerun/layout.json \
  --operbox target/codex-runs/<task>/private-rerun/operbox.json \
  --assignment target/codex-runs/<task>/private-rerun/team_shift_2.json \
  --explain-json > target/codex-runs/<task>/private-explain.json
```

诊断 envelope 当前为 `schema_version=1`，包含：

- `rooms.trade/manufacture/power`：干员和正式 `TradeResult` / `ManuResult` / `PowerResult` 分量；空贸易/制造房显式为 `status=empty`。
- `control_trace.events`：operator、`buff_id`、phase、condition/selector/action 与实际 outcome。
- `control_trace.final_state`：中枢执行完后的非零状态池，例如 Passion。
- `totals`：本次固定编制三设施域的效率和。

GlobalInject 的 merge 由 `infra-core::global_resource` 按真实顺序解释：制造 static 候选先执行正值
max / 负值累加，再执行 count-scaled max；只有实际影响最终累加值的调整标为 `accumulated`，较小
正候选仍为 `suppressed`，selector 结果为 0 固定为 `no_effect`。其余 max 槽使用 `winner`、
`tied_winner`、`suppressed`。当前贸易房计数、制造房标签和灵知精密计算分别输出
`deferred_to_trade_room`、`deferred_to_manufacture_room`、`special_rule`，不会伪装成 max loser。
未被中枢 interpreter 建模的 condition 保持现有 assumed-true 行为，同时明确标记为
`unhandled_assumed_true`。

该输出解释“已选 assignment 如何结算”，不解释未选候选为何落败；贸易/制造房内 EffectAtom
仍只提供聚合 solver breakdown。输出含私人编制和干员名单，必须重定向到私有 evidence，不提交。

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
| `--rotation` | 可选。`2` / `3` / `12/12/12` / `fiammetta-8844` / `abyssal-7575`；省略为 `3`，裸 `4` 和自由时长数组报错 |
| `--no-fiammetta` | 可选。关闭菲亚梅塔主力回岗覆盖；关闭后 MAA 输出的 `Fiammetta.enable` 全部为 `false`。与 `--rotation fiammetta-8844` 组合直接硬错误 |
| `--maa-out` | 写出 MAA 基建排班 JSON |
| `--profile-out` | 可选。账号画像 JSON 路径（默认同目录 `*_profile.json`） |
| `--output-dir` | 可选。按最终 2 / 3 / 4 个状态写出 `team_shift_*.json` assignment |
| `--baseline` | 可选。对比用基准 operbox（默认 `data/box_profile_knightcode.json`） |
| `--top` | Top-K 搜索条数，默认 20 |
| `--prefer system=alternative` | 可重复。优先尝试声明式规则 alternative；不可行时按规则顺序回退。未知 rule/alternative 会明确报错；例如 `--prefer rosemary_perception=recruit_refresh_witch` |
| `--maa-title` | 覆盖 MAA JSON 顶层 `title` |
| `--json` | 仅输出 profile JSON 到 stdout（跳过人类可读表） |

---

## 跑一遍模拟（Agent 默认）

> **给 Cursor / 协作者**：用户说「跑一遍模拟」「跑模拟」「三班模拟」等，且未指定其他命令时，**默认**跑 **`plan`** 并**写出 MAA JSON**，因为它同时包含账号分析与 αβγ 排班。只有用户明确说“仅排班 / 不要分析”时，才用 `layout team-rotation`。`layout test` 只用于单班搜索探测；A-B-A 入口已移除。

**无用户指定路径时，Agent 固定用：**

| 项 | 路径 |
|----|------|
| 布局 | `data/fixtures/243/layout.json` |
| 练度盒 | `data/fixtures/243/operbox_full_e2.json`（243 三班干员合法最高练度，均解锁 `tier_up`） |
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
| `--rotation` | 可选；显式 profile 是 hard constraint，不会静默回退默认三班 |
| `--no-fiammetta` | 可选。关闭菲亚梅塔回岗覆盖；与 `--rotation fiammetta-8844` 组合直接硬错误 |
| `--maa-out` | **Agent 默认必带**。写出 MAA 基建排班 JSON；默认 `out/243_maa.json` |
| `--maa-title` | 可选。覆盖 JSON 顶层 `title` |
| `--top` | Top-K 搜索条数，默认 20 |
| `--prefer system=alternative` | 可重复；三个入口均透传到同一规则编译器。语义是“优先尝试、不可行则回退”，未知值报错 |
| `--prefix-shadow-out <json>` | `plan` / `layout team-rotation` 可选；保留默认 V0 结果，另跑 V1 `1:1` 候选并记录房间、效率、dependency、耗时与失败阶段。结果状态为 `BEST_FOUND`；shadow 不写 MAA，计算、比较或写出失败都不替换或中断公开结果 |
| `--text` | 可选。带 `--maa-out` 时 stderr 默认已有人类可读排班表 |

`OperBox` 中每个条目必须提供 `1..=6` 的星级；已拥有条目的精英阶段与等级必须在该星级可达范围内。未知星级或不可能练度会直接报错，不会降档或自动修正。

### 输出约定

| 流 | 说明 |
|----|------|
| **stderr** | 所选 profile 的队伍花名册、轮换表、各班设施上岗与加权产出 |
| **`--maa-out` 文件** | MAA 协议 JSON（见 [FRONTEND_CLI.md](FRONTEND_CLI.md) §6） |

用户指定 `--layout` / `--operbox` / `--maa-out` 时以用户为准。MAA 映射实现见 `crates/infra-core/src/export/maa.rs`。

---

## 自定义布局 + 练度盒测试（改机制 smoke test）

> **给 Cursor / 协作者**：用户给出「某布局 JSON + operbox / 练度表」要跑**单班贸易/制造搜索探测**时，**优先用 `layout test`**，不要用 `bench`（`bench` 布局锁死 243c 基准）、也不要在 CLI 里临时拼 `LayoutContext` / 搜索上下文。若用户只说「跑一遍模拟」，见上节 **`plan`**。
>
> **无用户指定文件时，Agent 默认固定用：**
> - 布局：`data/fixtures/243/layout.json`
> - 练度盒：`data/fixtures/243/operbox_full_e2.json`（243 三班干员合法最高练度，均解锁 `tier_up`）

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
  [--text | --json]
```

用户指定路径时，将 `--layout` / `--operbox` 换成用户文件即可：

```bash
cargo run -p infra-cli -- layout test \
  --layout <蓝图.json> \
  --operbox <练度盒.json> \
  [--top <n>] \
  [-o <file.csv>] \
  [--text | --json]
```

| 参数 | 说明 |
|------|------|
| `--layout` | **必填**。任意路径的 `BaseBlueprint` JSON；**Agent 默认** `data/fixtures/243/layout.json` |
| `--operbox` | **必填**。玩家练度盒 JSON（`OperBox`）；**Agent 默认** `data/fixtures/243/operbox_full_e2.json`（合法最高练度）；用户自有练度或 `data/operbox_gongsun.json` 仅在用户指定时用 |
| `--top` | Top-K 条数，默认 3 |
| `-o` / `--output` | 写 CSV（UTF-8 BOM）；缺省 stdout |
| `--text` | 人类可读摘要写 stderr（**Agent 本地探测时推荐**） |
| `--json` | 输出 `{meta, trade, manufacture}`；制造最终效率为 `manufacture.report.*.final_efficiency`，分解值均为直接小数效率 |

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
- 宿舍推荐同时填写 `dorm_beds`（MAA 宿舍容量）和 `dorm_ambience_level`（“每间宿舍每级”技能读取值；满宿舍通常为 5）；`dorm_ambience_level` 缺失或为 0 时布局校验失败，`dorm_beds` 不参与效率计算。
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
| `main.rs` | `pool_cmd` / `search_cmd` / `trade_cmd` / `bench_cmd` |
| `output.rs` | `emit_pool`、`emit_trade_search`、`emit_bench`、`emit_trade_yield`、`emit_team_rotation` |

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
