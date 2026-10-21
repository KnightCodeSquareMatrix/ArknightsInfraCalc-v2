# infra-cli 模块职责与命令地图

> 文档角色：canonical
> 生命周期状态：current
> 领域键：architecture.cli
> 当前真源：self
> 摘要：说明 infra-cli 的分层职责、命令入口和 core 调用边界
> 代码范围：`crates/infra-cli/src/main.rs`、`crates/infra-cli/src/commands/`、`crates/infra-cli/src/output.rs`、`crates/infra-cli/src/verify/`
> 代码锚点：`main::run`、`commands::{plan_cmd,serve_cmd,layout_cmd,debug_cmd,verify_cmd,bake_cmd,profile_cmd}`、`output::*`
> 关联链路：[CLI 与 Worker](接口/CLI与Worker.md)、[plan 请求](调用链/plan请求.md)、[错误与诊断](接口/错误与诊断.md)
> 核验基线：`cargo run -q -p infra-cli -- --help`；各子命令 `--help`

`infra-cli` 是适配层：解析命令行或 JSONL，加载输入和运行时数据，调用 `infra-core`，再把结果写成文本、CSV、JSON 或 MAA 文件。它不拥有游戏机制公式、候选规则或评分政策。

## 进程分层

```text
argv / stdin JSONL
        │
        ▼
infra-cli commands ── 输入校验、默认值、文件 I/O、协议和输出
        │
        ▼
infra-core ────────── OperBox / layout / pool / search / plan / rotation / solve / export
        │
        ▼
stdout / stderr / JSON / CSV / MAA
```

| 目录 | 责任 | 不应承载 |
|---|---|---|
| `main.rs` | 子命令分发、全局 `--no-perf` 处理、旧 `pool/search/trade/bench` 入口 | 机制公式 |
| `commands/plan.rs` | 文件型 `plan` 参数和输出编排 | profile 算法、排班选择 |
| `commands/plan_compute.rs` | `plan` 与 Worker 共享的一次 rotation/profile/MAA 编排 | argv、JSONL envelope、机制公式 |
| `commands/serve.rs` | 常驻 JSONL、schema 校验、`ping`、`plan`、`plan.compute` | 第二套 solver |
| `commands/layout.rs` | layout test/analyze/eval/team-rotation 适配 | 房间结算公式 |
| `commands/debug.rs` | debug bundle 快照选择、当前 runtime 重跑、稳定 projection | 旧命令执行、bit-identical 断言 |
| `commands/verify.rs` + `verify/` | CSV 期望、固定 fixtures、回归断言 | 修改 core 行为 |
| `commands/bake.rs` | catalog 生成/validate/verify | 把 bake 当成业务真源 |
| `commands/profile.rs` | 性能和依赖报告 | 用户产品结果 |
| `output.rs` | 文本、CSV、profile/rotation 报告序列化 | 调 solver、重新排序 |

## 命令入口

完整输入输出和协议见 [接口/CLI与Worker.md](接口/CLI与Worker.md) 与 [接口/输入输出与兼容.md](接口/输入输出与兼容.md)。

| 命令 | 入口 | 核心调用 |
|---|---|---|
| `plan` | `plan_cmd` | `compute_plan` → `schedule_timed_rotation_request` → profile/MAA |
| `serve` | `serve_cmd` | `handle_request` → `handle_plan`/`handle_plan_compute` |
| `layout test` | `layout_test_cmd` | `assign_shift`/`resolve_base` 或固定 assignment |
| `layout team-rotation` | `layout_team_rotation_cmd` | `schedule_timed_rotation_request` → `build_from_team_rotation` |
| `layout analyze` | `layout_analyze_cmd` | `build_box_profile` |
| `layout eval` | `layout_eval_cmd` | `resolve_base` → 正式设施 solver → JSON/text |
| `debug rerun` | `rerun_cmd` | bundle snapshot → `compute_plan` → behavior projection |
| `verify` | `verify_cmd` | CSV → fixture → `solve_trade_with_shift` |
| `bake` | `bake_cmd` | `bake_catalogs` / `validate_baked_catalog` / response verify |
| `profile` | `profile_cmd` | 性能采样、依赖扫描 |

## 输出约定

- `plan` 默认 stdout 是人类报告，stderr 是路径、输入标签和性能；`--json` 只输出 profile JSON。
- Worker stdout 永远是一行一个 JSON response；人类日志只走 stderr。
- CSV 默认写文件时带 UTF-8 BOM；`--text` 用于人类可读输出。
- profile、rotation、MAA 和 explain JSON 都应由对应结构体序列化；消费者不得解析历史 stdout。

## 改动定位

新增命令改 `commands/foo.rs` 并在 `main::run` 注册；表格列和中文标签改 `output.rs`；输入字段改 contracts/schema 与对应 adapter；solver 结果错误沿 `infra-core` owner 定位。CLI 层只能修参数、协议、路径或输出投影问题。
