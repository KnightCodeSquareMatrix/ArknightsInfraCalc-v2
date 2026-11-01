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

当前 Worker 支持 v1–v7。本地最终release已验证ping、v7 ABC8/8/8、主备8/16及未知方法错误后正常退出；消费者以实际 `ping` 和契约 hash 为准。

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

`serve_cmd` 启动时加载运行时表、编译七个 `plan.compute` JSON Schema，并计算每个 schema 的 SHA-256 与当前可执行文件指纹。随后逐行读取 stdin：

```text
JSON request line → handle_request → method handler → JSON response line
```

支持 `ping`、`plan`、`plan.compute`。当前 `ping` 返回协议版本、支持的 schema 版本（v1–v7）、每版本契约 hash、solver 可执行文件 hash、构建 commit 和时间；部署消费者仍须以实际响应为准。`plan.compute` v7 保留 v6 named rotation 字符串，并增加普通主备/ABC 的 topology + `shift_hours` 对象；v1–v6 schema 文件与 wire 含义不变，v6 保持闭集。v3–v5 历史 schema 可结构解析旧龙巫 wire，但解码后会在求解前明确返回 `PLAN_FAILED`，不等于该 profile 仍受支持，也不会走普通主备 fallback；v1/v2 未包含该枚举。空白行跳过；EOF 正常退出；单行超过 8 MiB 返回 `FRAME_TOO_LARGE` 后继续读取下一行；非 UTF-8 返回 `INVALID_UTF8`。

Worker stdout 保持 JSONL。普通排班质量问题随已完成 rotation 的 warnings 返回，profile、MAA、rotation summary 和练卡建议读取同一完整结果。半成品不进入练卡建议；`PartialRotation` 仅保留已完成报告之后真实评估故障的诊断用途。协议、输入、资源/执行故障与显式 assert-invariants 失败沿原 error/PLAN_FAILED 边界，详见[错误与诊断](错误与诊断.md)。

## 责任边界

普通定时 `plan`、`layout team-rotation`、`layout test` 和 `layout analyze` 使用 `--enable-support-combinations` 显式开启主动挂件组合，未指定默认关闭。Worker `plan` 使用顶层布尔参数 `support_combinations_enable`；`plan.compute` v6–v7 使用 `options.support_combinations_enable`，所有入口缺省均为 `false`。`debug rerun` 源码支持v7 topology时长对象；本轮未单独运行该入口，不以Worker通过证明重跑覆盖。旧包未记录该字段时按当前默认关闭重跑，结果差异不等同于旧二进制回归。记录了已删除龙巫 profile 的旧 bundle 会明确失败，不能静默变成普通主备。

- `commands/*.rs` 负责参数、文件和协议适配，调用 `infra-core`；
- `commands/plan_compute.rs` 是 `plan` 与 `plan.compute` 共用的单次编排入口；
- `output.rs` 负责文本、CSV、JSON 序列化，不重新选人、不重算效率；
- `infra-core` 负责输入模型、候选、编排、轮换、机制结算和导出数据；
- `data/` 是运行时载体，不能反向裁决领域语义。

排查结果错误时，先判断是参数/加载、共享编排、core 结算还是输出投影，避免在 CLI 层修机制。

## 自动排班入口

`plan --rotation auto@H1/H2` 通过公共模式/时长请求解析后分派 `plan.rs::auto_rotation_cmd`，只加载输入、调用core并输出排班。支持 `--no-fiammetta`、`--operbox`、`--layout`、`--top`、`--prefer`、`--rotation-out`、`--output-dir`、`--assert-invariants`、`--json`；JSON在显式路径写入（output-dir下为auto_rotation.json），stdout默认中文逐站排班，`--json`时只输出排班JSON。逐站显示主力/替补、普通效率或充能/支援速度/中枢贸易与各制造配方及办公会客加成、窗口、缺额，常驻宿舍挂件单列。拒绝profile、MAA、baseline、prefix shadow和菲亚候选输出；菲亚按静态优先级配置，支持`--no-fiammetta`且gate核对请求开关。宿舍1＋1不受主动支持总开关影响。auto gate在写文件前检查最终两态；缺员是成功的不完整排班。正常工作技能候选在初始建池按对应设施及H1/H2试算，随机Fallback不做心情或技能检查；后续心情不足保留正常结果与质量提示，显式gate核对提示准确性以及身份、Plan和快照。不再证明任意零散成员或主备混搭不存在偶发跨站协同（保证边界见`docs/自动轮换.md` §2.10）。第一次主力完成后固定；合法替补候选耗尽保留真实缺额并输出当前排班，不回选主力或缩短时长。Worker plan_compute仍为定时契约；Check通过此独立CLI入口接自动排班。ping新增`auto_rotation.fiammetta_policy="static_priority"`能力声明，旧bundle未声明时不得静默接收开启请求。MAA未扩展。

自动排班明确输入主力/替补两段时长，裸 `auto` 返回补时长错误；例如 `auto@24/12`。输出 `timing.shift_hours`保存输入，两态的`evaluation_hours`分别为H1/H2，ETA首人只解释可工作时间。CLI模式/时长解析共用core请求，Worker仍只接受既有定时合同。

自动轮换 `plan --rotation auto@H1/H2` 现与普通 plan 共用性能 trace；标签为实际布局和 Box 路径，guard 包含数据加载、求解及终态验收，退出时输出 schema2 汇总。阶段耗时仍为包含子调用的口径，不能相加；失败和提前返回也保留汇总。

## 本地验收范围（2026-10-31）

最终release以隔离schema17完整表启动Worker，ping和两类v7普通拓扑请求均成功且开启终态断言；返回班时为8/8/8和8/16。未知方法返回单帧错误，EOF进程exit0。证据见`target/codex-runs/main-integration-20261031/worker-final-live/`。138s是含完整表冷加载、与Bake后校验并行的进程总时长，不作为热请求速度。未部署，也未覆盖所有历史schema、所有错误帧或debug rerun。制品以实际SHA关联源码冻结树；嵌入构建commit/时间可能来自Cargo先前缓存，本轮记录实际值，不把它当成当前源码HEAD证明。
