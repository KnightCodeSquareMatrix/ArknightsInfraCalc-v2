# CLI 与 Worker 接口入口

> 文档角色：canonical
> 生命周期状态：current
> 领域键：interface.cli-worker
> 当前真源：self
> 摘要：定位 infra-cli 命令、常驻 Worker 方法、标准流与代码入口
> 代码范围：`crates/infra-cli/src/main.rs`、`crates/infra-cli/src/commands/`
> 代码锚点：`crates/infra-cli/src/main.rs::run`; `crates/infra-cli/src/commands/serve.rs::serve_cmd`; `crates/infra-cli/src/commands/plan.rs::plan_cmd`; `crates/infra-cli/src/commands/plan_compute.rs::compute_plan`
> 关联链路：`docs/调用链/plan请求.md`; `docs/接口/错误与诊断.md`; `docs/接口/输入输出与兼容.md`
> 核验基线：`infra-cli --help`、`infra-cli serve --help`；`plan --help` 明确拒绝且不得触发求解或 profile 写入

本页只记录接口和模块责任。游戏机制由机制知识库提供；求解器的候选、编排和结算行为由 `infra-core` 的领域文档与代码提供。

本次提交包含 Worker v7 WIP 源码与契约，但实现状态仍为 `in-progress`，未完成 fresh release 验收或部署；消费者必须以实际 `ping` 和契约 hash 为准。

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

`plan_cmd` 会在读取路径和加载输入前拒绝 `--help`；`-h` 也会被拒绝，只有作为 `--maa-title` 的值时例外。该入口不提供帮助输出。通过参数检查后才加载 `BaseBlueprint`、`OperBox`、`OperatorInstances`、`SkillTable` 和 baseline，再调用共享 `plan_compute::compute_plan`。它不会实现机制公式。成功后：

- stdout：账号画像和轮换的可读报告；`--json` 时只打印 profile JSON；
- stderr：`layout=`、`operbox=`、阶段性能记录以及 profile/rotation/MAA 文件路径；
- 文件：profile 默认写到 operbox 同目录，`--profile-out` 可覆盖；`--maa-out`、`--rotation-out`、`--output-dir` 显式请求才写入。

`--no-fiammetta` 将排班请求的 `fiammetta_enable` 置为 false；普通定时轮换不得产生菲亚动作。Worker `plan.compute` 使用 `options.fiammetta_enable`（缺省为 true）和 `options.assert_invariants` 表达相同请求。两者开启终态检查时都会把请求开关与最终 rotation 一起核对，不能只检查动作是否 ready。

`--prefer rosemary_perception=recruit_refresh_witch` 已不再是可用值；当前感知体系可选 `recruit_refresh`、`high_perception`、`recruit_refresh_zilan`，不可行时仍按规则回退。独立“龙巫二班”已删除，CLI 不列出其旧别名，显式传入旧 wire/别名会返回不支持错误，不降级成主备排班。`--assert-invariants` 在共享编排完成后调用 request-aware `assert_final_rotation_invariants_for_request`，将 `fiammetta_enable` 请求与最终班次动作一并核对；通用无请求入口 `assert_final_rotation_invariants` 保留给兼容调用方。其余检查包含最终班次黑键与巫恋贸易禁同站及 `rotation_bind` 工作/支持统计。宿舍入住不算绑定工作班；真实拆组、班数或支持缺席在普通产品中保留排班及 warning，在显式验收中返回错误。断言失败时命令失败；这不是输出层错误。

## 常驻 Worker

`serve_cmd` 当前 WIP 源码启动时加载运行时表、编译七个 `plan.compute` JSON Schema，并计算每个 schema 的 SHA-256 与当前可执行文件指纹；本次提交一并记录 v7 机器契约，但未完成 fresh release 验收或部署。随后逐行读取 stdin：

```text
JSON request line → handle_request → method handler → JSON response line
```

支持 `ping`、`plan`、`plan.compute`。本次 WIP 源码的 `ping` 返回协议版本、支持的 schema 版本（v1–v7）、每版本契约 hash、solver 可执行文件 hash、构建 commit 和时间；部署消费者仍须以实际响应为准。`plan.compute` v7 保留 v6 named rotation 字符串，并增加普通主备/ABC 的 topology + `shift_hours` 对象；v1–v6 schema 文件与 wire 含义不变，v6 保持闭集。v3–v5 历史 schema 可结构解析旧龙巫 wire，但解码后会在求解前明确返回 `PLAN_FAILED`，不等于该 profile 仍受支持，也不会走普通主备 fallback；v1/v2 未包含该枚举。空白行跳过；EOF 正常退出；单行超过 8 MiB 返回 `FRAME_TOO_LARGE` 后继续读取下一行；非 UTF-8 返回 `INVALID_UTF8`。

Worker stdout 必须保持 JSONL，不能混入人类日志。请求解析、未知方法和协议错误没有业务 result；`plan.compute` 的求解失败通常在外层 `error` 同时保留结构化 result，详见[错误与诊断](错误与诊断.md)。

## 责任边界

普通定时 `plan`、`layout team-rotation`、`layout test` 和 `layout analyze` 使用 `--enable-support-combinations` 显式开启主动挂件组合，未指定默认关闭。Worker `plan` 使用顶层布尔参数 `support_combinations_enable`；`plan.compute` v6–v7 使用 `options.support_combinations_enable`，所有入口缺省均为 `false`。本次 v7 WIP 允许 `debug rerun` 携带 topology 时长对象，但未完成 fresh release 验收。旧包未记录该字段时按当前默认关闭重跑，结果差异不等同于旧二进制回归。记录了已删除龙巫 profile 的旧 bundle 会明确失败，不能静默变成普通主备。

- `commands/*.rs` 负责参数、文件和协议适配，调用 `infra-core`；
- `commands/plan_compute.rs` 是 `plan` 与 `plan.compute` 共用的单次编排入口；
- `output.rs` 负责文本、CSV、JSON 序列化，不重新选人、不重算效率；
- `infra-core` 负责输入模型、候选、编排、轮换、机制结算和导出数据；
- `data/` 是运行时载体，不能反向裁决领域语义。

排查结果错误时，先判断是参数/加载、共享编排、core 结算还是输出投影，避免在 CLI 层修机制。

## 自动建议入口

`plan --rotation auto@H1/H2` 通过公共模式/时长请求解析后分派 `plan.rs::auto_rotation_cmd`，只加载输入、调用core并输出建议。支持 `--operbox`、`--layout`、`--top`、`--prefer`、`--rotation-out`、`--output-dir`、`--assert-invariants`、`--json`；JSON在显式路径写入（output-dir下为auto_rotation.json），stdout默认中文逐站建议，`--json`时只输出建议JSON。逐站显示主力/替补、普通效率或充能/支援速度/中枢贸易与各制造配方及办公会客加成、窗口、缺额，常驻宿舍挂件单列。拒绝profile、MAA、baseline、prefix shadow和菲亚候选输出；菲亚固定关闭。宿舍1＋1不受主动支持总开关影响。auto gate在写文件前检查最终两态；缺员且预算、身份与显式Plan关系合法时是成功的不完整建议；工时/结构失败是错误。不再证明任意零散成员或主备混搭不存在偶发跨站协同（保证边界见`docs/自动轮换.md` §2.10）。第一次主力完成后固定，替补不能兼容时明确返回当前固定主力下未找到方案，不回选主力或缩短时长。Worker schema、plan_compute、MAA及网站未扩展。

自动建议明确输入主力/替补两段时长，裸 `auto` 返回补时长错误；例如 `auto@24/12`。输出 `timing.shift_hours`保存输入，两态的`evaluation_hours`分别为H1/H2，ETA首人只解释可工作时间。CLI模式/时长解析共用core请求，Worker仍只接受既有定时合同。
