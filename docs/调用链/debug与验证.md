# Debug、验证与证据调用链

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/接口/错误与诊断.md
> 机制依赖：docs/机制/知识库引用.md；vault:docs/1-基础设定/README.md
> 摘要：按错误症状选择复现、固定编制解释、生产 bundle 重跑和回归入口
> 代码范围：`crates/infra-cli/src/commands/{debug.rs,verify.rs,layout.rs,profile.rs}`、`crates/infra-core/src/{layout,trade,manufacture,schedule,bake}`
> 代码锚点：`crates/infra-cli/src/commands/debug.rs::rerun_cmd`; `crates/infra-cli/src/commands/layout.rs::layout_eval_cmd`; `crates/infra-cli/src/commands/verify.rs::verify_cmd`; `crates/infra-cli/src/commands/verify.rs::run_regressions`; `crates/infra-core/src/schedule/team_rotation.rs::assert_final_rotation_invariants`
> 关联链路：`docs/调用链/plan请求.md`; `docs/接口/错误与诊断.md`
> 核验基线：输入级最小复现；固定 assignment；代表性 243 plan；适用时 `verify --all`

## 选择入口

| 你观察到的现象 | 首选入口 | 能证明什么 |
|---|---|---|
| 生产 bundle 与当前结果不同 | `debug rerun --bundle ...` | 当前 runtime 对同一快照的稳定 projection；不证明旧环境 bit-identical |
| 已知某个 assignment 的效率/注入错误 | `layout eval --explain-json` | 已选编制如何结算；不解释未选候选为何落败 |
| 某个 trade shortcut 或单位产出错误 | `verify --case <id>` / `trade yield <fixture>` | CSV 期望与正式 trade solver 的回归结果 |
| 轮换、容量、bind、最终状态错误 | `plan --assert-invariants` | 当前 Plan 入口和最终 rotation invariant；`layout team-rotation` 不解析此开关 |
| 搜索慢或 bake 可疑 | `profile ...`、真实 live/Bake CLI 对比 | 阶段耗时和结果差分；Bake 不再自动校验 |

## `debug rerun`

`commands/debug.rs::rerun_cmd` 读取 bundle 的 inline layout/operbox 和 `serveRequest`。`plan.compute` 优先使用请求中的 inline snapshot；普通 `plan` 使用 bundle 顶层 snapshot，必要时才回退同目录 `layout.json`/`operbox.json`。bundle 中的旧 command、savedFiles、绝对路径和输出标签不会被执行。

它重新调用 `compute_plan`，为当前 executable 和运行时 catalog 计算 SHA-256，输出完整私有报告与脱敏 stdout 摘要。comparison 只比较 rotation、daily 三域、班次时长/transition 和房间有序成员；显式覆盖 `--top` 或 `--rotation` 时标为不可比较。`--room`/`--operator` 会暴露私有编制，必须和新的 `--output-dir` 一起使用。

## `layout eval --explain-json`

`layout_eval_cmd` 加载 layout、operbox 和指定 assignment，调用 `resolve_base` 与各房 solver；`--explain-json` 额外打开中枢 L1 trace。输出当前版本 `schema_version=1`，包含贸易/制造/发电 room result、control events/final state 和 totals。它解释“这个 assignment 怎样结算”，不能替代搜索诊断，也不接受 `--text` 同时使用。

如果 assignment 本身不合法，先修输入或回到 rotation/Plan owner；不要把 explain 输出当作候选生成日志。

## `verify`

`commands/verify.rs::run_regressions` 从 `data/REGRESSION_CASES.csv` 和 `data/UNIT_OUTPUT_ANCHORS.csv` 读取期望，按 `rule_id`/case id 选择 `verify/fixtures.rs` 中的固定夹具，调用 `solve_trade_with_shift`，比较效率、机制等效效率、rule id 和单位产出。未接线的 case 会明确打印 `skip ... (fixture not wired)`；任一已执行 case 失败，命令返回 `regression failures`。

## 证据边界

每次诊断应保留：命令、输入路径或 hash、layout/operbox、rotation/top、数据目录、当前 commit、完整错误、首个失败阶段和结果分类。区分：

- `input_failure`：加载/解析/语义校验失败；
- `solver_failure`：候选或约束导致主动无解；
- `timeout_failure`：规定时间内未完成；
- `invariant_failure`：结果生成后最终状态断言失败。

一次成功 `plan` 只能说明该输入在该 runtime 下成功；不能证明全局最优、全量机制覆盖或所有 fixture 通过。游戏机制问题引用知识库机制页，solver 行为问题沿本页的代码锚点定位。
