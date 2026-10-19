# CLI 与 Worker 接口入口

> 文档角色：canonical
> 生命周期状态：current
> 领域键：interface.cli-worker
> 当前真源：self
> 摘要：定位 infra-cli 命令、常驻 Worker 方法、标准流与代码入口
> 代码范围：`crates/infra-cli/src/main.rs`、`crates/infra-cli/src/commands/`
> 代码锚点：`crates/infra-cli/src/main.rs::run`; `crates/infra-cli/src/commands/serve.rs::serve_cmd`; `crates/infra-cli/src/commands/plan.rs::plan_cmd`; `crates/infra-cli/src/commands/plan_compute.rs::compute_plan`
> 关联链路：`docs/调用链/plan请求.md`; `docs/接口/错误与诊断.md`; `docs/接口/输入输出与兼容.md`
> 核验基线：`infra-cli --help`、`infra-cli serve --help`、`cargo run -q -p infra-cli -- plan --help`

本页只记录接口和模块责任。游戏机制由机制知识库提供；求解器的候选、编排和结算行为由 `infra-core` 的领域文档与代码提供。

## 用户可见入口

| 命令 | 代码入口 | 输入 | 输出或用途 |
|---|---|---|---|
| `plan` | `commands/plan.rs::plan_cmd` | `--operbox`、可选 `--layout`、轮换和输出路径 | 一次排班 probe；stdout 报告，stderr 元数据/路径，profile/MAA/rotation 可写文件 |
| `serve` | `commands/serve.rs::serve_cmd` | stdin 一行一个 JSON 请求 | stdout 一行一个 JSON 响应；stderr 只写日志和性能信息 |
| `layout test` | `commands/layout.rs::layout_test_cmd` | layout、operbox，可选 assignment | 单班搜索或固定编制检查 |
| `layout team-rotation` | `commands/layout.rs::layout_team_rotation_cmd` | layout、operbox、轮换 | 只做排班和可选 MAA，不生成账号 profile |
| `layout analyze` | `commands/layout.rs::layout_analyze_cmd` | layout、operbox、baseline | profile 分析，不等同于排班输出 |
| `layout eval` | `commands/layout.rs::layout_eval_cmd` | layout、operbox、assignment | 固定编制结算；`--explain-json` 输出结构化 trace |
| `debug rerun` | `commands/debug.rs::rerun_cmd` | 生产 debug bundle | 用当前 runtime 重跑并输出稳定行为投影；不是旧二进制重放 |
| `verify` | `commands/verify.rs::verify_cmd` | case id 或 `--all` | 回归夹具与单位产出锚点断言 |
| `bake` | `commands/bake.rs::bake_cmd` | all/trade/manufacture、输出目录 | 生成/验证本地候选 catalog；不改变 solver 语义 |
| `profile` | `commands/profile.rs::profile_cmd` | 性能分析参数 | 开发诊断；不提供产品结果 |

主进程由 `main::run` 分发。未知命令返回错误并退出失败；没有子命令时只打印 usage 并成功返回。

## `plan` 的标准流

`plan_cmd` 读取路径、加载 `BaseBlueprint`、`OperBox`、`OperatorInstances`、`SkillTable` 和 baseline，然后调用共享 `plan_compute::compute_plan`。它不会实现机制公式。成功后：

- stdout：账号画像和轮换的可读报告；`--json` 时只打印 profile JSON；
- stderr：`layout=`、`operbox=`、阶段性能记录以及 profile/rotation/MAA 文件路径；
- 文件：profile 默认写到 operbox 同目录，`--profile-out` 可覆盖；`--maa-out`、`--rotation-out`、`--output-dir` 显式请求才写入。

`--prefer rosemary_perception=recruit_refresh_witch` 已不再是可用值；当前感知体系可选 `recruit_refresh`、`high_perception`、`recruit_refresh_zilan`，不可行时仍按规则回退。独立“龙巫二班”已删除，CLI 不列出其旧别名，显式传入旧 wire/别名会返回不支持错误，不降级成主备排班。`--assert-invariants` 在共享编排完成后调用 `assert_final_rotation_invariants`，其中包含最终班次黑键与巫恋贸易禁同站及 `rotation_bind` 工作/支持统计检查。宿舍入住不算绑定工作班；真实拆组、班数或支持缺席在普通产品中保留排班及 warning，在显式验收中返回错误。断言失败时命令失败；这不是输出层错误。

## 常驻 Worker

`serve_cmd` 启动时加载运行时表、编译六个 `plan.compute` JSON Schema，并计算每个 schema 的 SHA-256 与当前可执行文件指纹。随后逐行读取 stdin：

```text
JSON request line → handle_request → method handler → JSON response line
```

支持 `ping`、`plan`、`plan.compute`。`ping` 返回协议版本、支持的 schema 版本（v1–v6）、每版本契约 hash、solver 可执行文件 hash、构建 commit 和时间。`plan.compute` 当前用 v6；v1–v5 历史 schema 仍可结构解析旧龙巫 wire，但解码后会在求解前明确返回 `PLAN_FAILED`，不等于该 profile 仍受支持，也不会走普通主备 fallback。空白行跳过；EOF 正常退出；单行超过 8 MiB 返回 `FRAME_TOO_LARGE` 后继续读取下一行；非 UTF-8 返回 `INVALID_UTF8`。

Worker stdout 必须保持 JSONL，不能混入人类日志。请求解析、未知方法和协议错误没有业务 result；`plan.compute` 的求解失败通常在外层 `error` 同时保留结构化 result，详见[错误与诊断](错误与诊断.md)。

## 责任边界

`plan`、`layout team-rotation`、`layout test` 和 `layout analyze` 使用 `--enable-support-combinations` 显式开启主动挂件组合，未指定默认关闭。Worker `plan` 使用顶层布尔参数 `support_combinations_enable`；`plan.compute` v6 使用 `options.support_combinations_enable`，所有入口缺省均为 `false`。`debug rerun` 接受 v1–v6，并保留请求开关；旧包未记录该字段时按当前默认关闭重跑，结果差异不等同于旧二进制回归。记录了已删除龙巫 profile 的旧 bundle 会明确失败，不能静默变成普通主备。

- `commands/*.rs` 负责参数、文件和协议适配，调用 `infra-core`；
- `commands/plan_compute.rs` 是 `plan` 与 `plan.compute` 共用的单次编排入口；
- `output.rs` 负责文本、CSV、JSON 序列化，不重新选人、不重算效率；
- `infra-core` 负责输入模型、候选、编排、轮换、机制结算和导出数据；
- `data/` 是运行时载体，不能反向裁决领域语义。

排查结果错误时，先判断是参数/加载、共享编排、core 结算还是输出投影，避免在 CLI 层修机制。
