# 第一次完整运行：243 全精二方案

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/BASE_ASSIGNMENT.md；docs/SCHEDULE_ROTATION.md
> 摘要：带新读者从仓库夹具运行第一套完整方案并检查输出

> 用途：面向第一次运行项目的读者，完成输入、求解、结果阅读和产物检查。
> 本页描述当前 CLI 能力和 fixture，不定义任何体系的业务不变量。

本教程使用仓库自带的 243 布局和最高合法练度盒，运行一次完整 `plan`，得到人类可读排班、账号画像 JSON 和 MAA 排班 JSON。历史文件名 `full_e2` 为兼容保留；低星干员使用可达的最高精英阶段，所有条目均解锁 `tier_up`。无需先理解内部架构；完成后再按兴趣进入 [项目总览](../OVERVIEW.md) 或 [架构导览](../ARCHITECTURE_TOUR.md)。

## 1. 运行前准备

在仓库根目录执行命令，并确保本机 Rust / Cargo 工具链可以构建 workspace。Python 只用于下面可选的 JSON 语法检查，不参与 solver。

| 输入 | 路径 | 说明 |
|------|------|------|
| 243 layout | [data/fixtures/243/layout.json](../../data/fixtures/243/layout.json) | 默认 243 测试布局 |
| 最高合法练度 operbox | [data/fixtures/243/operbox_full_e2.json](../../data/fixtures/243/operbox_full_e2.json) | 仓库标准 `tier_up` 练度盒 |
| 账号画像参考 | [data/fixtures/243/schedule_export.json](../../data/fixtures/243/schedule_export.json) | profile 对比参考，不是当前 solver 的强制排班 |

先准备输出目录：

```bash
mkdir -p out
```

## 2. 运行完整方案

```bash
cargo run -q -p infra-cli -- plan \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/243-full-e2-profile.json \
  --maa-out out/243-full-e2-maa.json
```

首次构建可能比后续运行慢。成功标准是命令 exit code 为 0，且末尾能看到三班排班和每日加权效率；编译 warning 或运行期提示本身不等于失败。

本教程显式传入所有输入，避免依赖默认值。实际账号也应显式传入自己的 layout、operbox 和独立输出路径；完整参数契约以 [infra-cli 文档](../INFRA_CLI.md) 为准。

维护者或 Agent 需要把结果作为交付证据时，应通过 [Evidence 工具](../../scripts/codex/README.md) 包装同一命令，并使用任务专属产物名。

## 3. 先读终端输出

stdout 按以下顺序展示：

1. 账号概览和与参考基准的分域对比。
2. 24 小时贸易、制造、发电加权结果。
3. 当前 rotation profile；本命令默认是 ABC `12h + 6h + 6h`。
4. alpha、beta、gamma 队伍花名册和三班上岗/休息关系。
5. 每班各设施成员、直接效率、时长加权效率和 peak 心情时间锚点。

先确认终端中的 `layout=` 和 `operbox=` 指向预期输入，再看“轮换一览”和每个 Shift。默认 ABC 的结构应为：

```text
shift1  12h  alpha+beta 上岗，gamma 休息
shift2   6h  beta+gamma 上岗，alpha 休息
shift3   6h  gamma+alpha 上岗，beta 休息
```

具体成员和效率会随规则、数据和 solver 改进而变化，不要用本页历史快照逐行比对当前 stdout。

stderr 包括加载提示、阶段计时、写出路径和 warning。最终判断以 exit code、结果摘要和产物检查为准。

## 4. 检查两个 JSON 产物

先确认两个文件存在且 JSON 语法有效：

```bash
python3 -m json.tool out/243-full-e2-profile.json >/dev/null
python3 -m json.tool out/243-full-e2-maa.json >/dev/null
```

### 4.1 Profile JSON

`out/243-full-e2-profile.json` 是账号画像与轮换指标快照，当前主要包含：

- `schema_version`
- `rotation_profile`
- `layout_label`、`operbox_label`、`baseline_label`
- `summary`、`domains`、`actions`、`flags`
- `rotation` 与 `baseline_rotation`
- `narration_hints`

字段形状以当前序列化类型和 [INFRA_CLI.md](../INFRA_CLI.md) 为准；消费者应检查 `schema_version`，不要假设历史 JSON 永久兼容。

Profile 保存分析、分域指标和 rotation 摘要，不是完整房间 assignment 的副本。

### 4.2 MAA JSON

`out/243-full-e2-maa.json` 当前顶层包含：

- `title`
- `description`
- `planTimes`
- `plans`

每个 plan 包含班次名称、说明、无人机、菲亚梅塔设置和 `rooms`；`rooms` 下按 trading、manufacture、power、control、dormitory、hire、meeting 等设施输出成员。

MAA 导出是当次 core 排班的映射，不应在导出层重新选择干员或改变班次语义。

`plan` 当前只生成一次用户 rotation；profile、最终 stdout 和 MAA 都消费该同一结果。验证时，stdout 中的工作房成员应与 MAA 对应班次一致；宿舍成员是休息安排，不能算作在岗 presence。

## 5. 这次运行证明了什么

使用标准输入，当前入口能够：

- 完成账号画像和参考基准对比。
- 运行 peak 单班编排、αβγ 三队轮换和逐班效率结算。
- 保持每班生产设施与中枢按当前约束满编，报告工作队和休息队。
- 将当前实际 assignment 输出为人类可读表并导出为 MAA JSON；profile JSON 保存账号分析与轮换指标快照，不是完整房间 assignment 的副本。
- 对已建模的跨设施资源、候选投影、shortcut 和实际 shift bind 进行当前实现范围内的求值。
- 在 baked catalog 不兼容时安全回退实时搜索。

这次成功运行不表示：

- 全仓库 full suite 当前全绿。
- 当前搜索已经实现全基建、全班次的全局最优。
- 当前所有体系都完成严格审计。
- 当前动态中枢 producer 已完成未来联合 baked search 计划。

当前单班编制属于领域约束下的分阶段、逐房搜索。准确的保证等级和验证要求见 [质量与审计](../QUALITY_AND_AUDIT.md)。

## 6. 换成自己的输入

保留命令形状，只替换输入和输出路径：

```bash
cargo run -q -p infra-cli -- plan \
  --layout path/to/layout.json \
  --operbox path/to/operbox.json \
  --profile-out out/my-profile.json \
  --maa-out out/my-maa.json
```

其他 timed rotation profile、`--output-dir`、单班探测和显式 assignment 复算见 [infra-cli 文档](../INFRA_CLI.md)。接口消费者应继续读 [前端对接说明](../FRONTEND_CLI.md)，不要把本教程的文件名或当前 lineup 当成协议。

## 7. 历史观察快照

以下历史结果来自代码基线 `dfe3cf0`、标准全精二 operbox 和本页默认命令。它早于共享单次 Plan 编排，只展示当时入口能产出什么，不代表当前或未来版本必须固定选择相同第三人、房号或班次。

| 项目 | 本次结果 |
|------|----------|
| operbox | 418 名，全部 `tier_up` |
| 可建模池 | 贸易 75，制造 90 |
| 轮换 | `12h α+β`、`6h β+γ`、`6h γ+α` |
| 24h 加权效率 | 贸易 5.181，制造 9.196，发电 3.552 |
| peak 体系 | `automation_group`、`human_fireworks_perception`、`pinus_sylvestris` |
| 当时第二次 rotation 耗时 | 7.48 秒（旧基线本机 debug `cargo run` 观察值，不是当前 benchmark） |

实际贸易与中枢快照：

| 班次 | 中枢 | 贸易站 1 | 贸易站 2 |
|------|------|----------|----------|
| Shift 1 / 12h | 令、八幡海铃、焰尾、薇薇安娜、重岳 | 伺夜、但书、黑键（2.883） | 贝洛内、乌有、可露希尔（2.432） |
| Shift 2 / 6h | 凛御银灰、斩业星熊、歌蕾蒂娅、玛恩纳、诗怀雅 | 卡夫卡、巫恋、龙舌兰（2.482） | 吉星、能天使、蕾缪安（2.293） |
| Shift 3 / 6h | 令、八幡海铃、焰尾、薇薇安娜、重岳 | 伺夜、但书、黑键（2.883） | 贝洛内、乌有、可露希尔（2.432） |

该快照也诚实暴露了下一步边界：动态 producer 已统一比较 presence 集合，但多房内部仍是有序填房 policy，尚未实现 exact joint；后续见 [DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](../TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

### 为什么快照不是业务规则

同一 fixture 的具体 lineup 会随以下内容变化：

- 业务 Markdown 裁决。
- skill / instance / shortcut / segment 数据。
- 候选池完整性和 tie-break。
- scoring policy、bake schema 和搜索算法。

因此不要把某次输出中的具体房号、队名、第三人或某个 top hit 写成测试常量。业务断言应来自领域 Markdown，例如：

- 必须存在的硬核心。
- 允许跨站或禁止同房的作用域。
- 实际 producer / consumer 的同上同下关系。
- 每班中枢人数和设施容量。
- 未入选候选不得被轮换层重新强塞。

如需记录当前 lineup，应将其标为“日期 + commit + 输入 + 命令”的观察快照，并保留对应日志和 JSON；它只能用于 diff 与复现，不能推翻领域真源。

## 8. 维护者验收清单

一次正式默认模拟至少检查：

1. 命令 exit code 为 0。
2. profile JSON 和 MAA JSON 均存在且可解析。
3. MAA `plans` 与当前轮换班次数一致。
4. 每个工作房成员不超过容量，同一班无重复干员。
5. 每班中枢按当前规则满 5 人。
6. 明确 bind 的成员实际 presence 一致。
7. stdout 的核心成员与同一次最终 rotation 生成的 MAA 工作房一致；宿舍休息成员不能误算为在岗。profile 只比较轮换指标，不比较完整房间 assignment。
8. 日志包含命令、输入、耗时、exit code 和结果摘要。

Full suite 有既有失败时，按 [QUALITY_AND_AUDIT.md](../QUALITY_AND_AUDIT.md) 比较精确失败集合；不得只因本次 `plan` 成功就声称项目全部通过。

### 常见误用

- 用 `layout test` 单班探测代替用户要求的完整模拟。
- 不传 `--profile-out` / `--maa-out`，覆盖 operbox 相邻文件或丢失证据。
- 把 MAA 宿舍中的休息干员算作工作 presence。
- 把一次全精二 top hit 固定为体系 hard core。
- 只检查 JSON 文件存在，不检查解析、班次数、中枢人数和核心成员。
- 把 profile JSON 当成最终 MAA assignment 的逐房序列化副本。
- 用旧 baked catalog 的结果判断当前实现；catalog mismatch 时应确认实时 fallback。
