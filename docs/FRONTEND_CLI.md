# infra-cli + Layout 生成器 — 前端对接说明

> 文档角色：canonical
> 生命周期状态：current
> 领域键：interface.frontend-cli
> 当前真源：self
> 摘要：裁决前端调用 CLI 和导出数据的接口契约

> 面向前端 / 排班 UI。**Layout 蓝图**用静态页 `layout-gen` 编辑；**排班求解 + MAA JSON** 用 `infra-cli`（子进程或后续 WASM）。
> **Beta Release 构建**：2026-06-25 · backend commit `9e52de9` · frontend beta source `3259eaa` · 见 `release/VERSION.txt`
> **Worker v1 落地状态（2026-07-23）**：后端与 Next BFF 的 `plan.compute` 实现及本地证据已完成；目标分支集成、发布部署和真实浏览器验收仍由 [Worker 内联 JSON 集成与部署验收](TODO/Worker内联JSON集成与部署验收.md) 跟踪。上述 Beta Release 不包含本轮迁移；已安装 Worker 是否支持 v1 以 `ping` 版本字段和契约 hash 为准。

---

## 1. Release 包内容

```
release/
├── infra-cli / infra-cli.exe  Linux / Windows x64 CLI
├── data/                      Linux 独立部署包内置；Windows 旧包可放在 release 同级
├── layout-gen/
│   └── index.html             基建 Layout 生成器（浏览器打开，无构建）
├── fixtures/
│   ├── layout.json            243 样例蓝图
│   └── operbox_full_e2.json   243 样例练度（各星级合法最高练度）
├── docs/
│   └── FRONTEND_CLI.md        本文件副本
├── plans/
│   └── cli-format-reference.md
├── README.md                  快速上手
└── VERSION.txt
```

Linux / macOS CLI：

```bash
cargo build --release -p infra-cli
# layout-gen/index.html 跨平台通用，无需编译
```

Linux 发布包：

```bash
bash scripts/build_release_linux.sh
```

---

## 2. Layout 生成器（`layout-gen`）

### 2.1 打开方式

```bash
xdg-open release/layout-gen/index.html
```

Windows 可在资源管理器双击，或运行 `start release/layout-gen/index.html`。

源码与 release 包同步：`tools/layout-gen/index.html`（单文件，内联 CSS/JS）。

### 2.2 能力

| 功能 | 说明 |
|------|------|
| 基建预设 | 243 / 153 / 333 / 252 / 342（贸/制/电数量） |
| 房间编辑 | 等级、贸易订单（LMD/合成玉）、制造配方、宿舍床位、宿舍有效等级 |
| 场景假设 | `drone_cap`、`sui_facility_count`、`dorm_occupant_count`、`monster_cuisine` 等；满清理基建 `drone_cap` 为 235，用于承曦格雷伊「巡线框架」 |
| 导出 | 下载 `BaseBlueprint` JSON → 作为 CLI `--layout` |
| 导入 | 加载已有 layout JSON 继续编辑 |

UI 参考一图流排班 V2 的 `room-wrap` 三区布局；**只负责蓝图，不算排班**。

### 2.3 与 CLI 的数据流

```
layout-gen 导出 JSON  ──→  --layout my_layout.json
用户 operbox JSON     ──→  --operbox operbox.json
infra-cli plan         ──→  --profile-out 账号画像 JSON + --maa-out MAA JSON
```

前端若做一体化产品：**Layout 页产出 JSON 字符串/文件 → 传给后端或本地 spawn CLI**，无需手存 `data/layout/`（除非用户要持久化）。

---

## 3. 运行前提：内置数据

`plan` / `layout team-rotation` 使用程序内置机制数据；前端传入 `--layout`、`--operbox`，需要非默认班制时再传 `--rotation`。

默认布局：243。
默认练度盒：full_e2。

`data/baked/` 仍然是本地 bake 缓存，不属于程序内置真源。

---

## 4. 主命令

### 4.0 `plan`（推荐：账号分析 + 排班一体）

```bash
infra-cli plan \
  --operbox <练度盒.json | 一图流.xlsx> \
  [--layout <蓝图.json>] \
  [--rotation <2 | 3 | 12/12/12 | fiammetta-8844 | abyssal-7575>] \
  [--maa-out <输出/schedule.json>] \
  [--profile-out <画像.json>]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `--operbox <path>` | 是 | `OperBox` JSON 数组，或一图流导出的 **xlsx** |
| `--layout <path>` | 否 | 默认 `data/fixtures/243/layout.json` |
| `--rotation <profile>` | 否 | 默认 `3`；可选 `2`、`12/12/12`、`fiammetta-8844`、`abyssal-7575`；裸 `4` 和自由时长数组硬错误 |
| `--maa-out <path>` | 否* | MAA 自定义基建排班 JSON；一次性 CLI 集成应传入已知临时路径 |
| `--profile-out <path>` | 否 | 账号画像 JSON；一次性 CLI 集成应传入已知临时路径 |
| `--baseline <operbox>` | 否 | 对比基准练度盒（画像用） |
| `--maa-title <text>` | 否 | 覆盖 MAA JSON 顶层 `title` |
| `--top <n>` | 否 | 搜索深度，默认 `20` |
| `--output-dir <dir>` | 否 | 额外写出每班 `team_shift_*.json` |
| `--json` | 否 | 仅输出账户画像 JSON 到 stdout（调试）；不包含 MAA JSON |

**输出约定（默认，无 `--json`）：**

| 流 | 内容 |
|----|------|
| **stdout** | 账号画像摘要 + 所选 rotation profile 的人类可读排班表 |
| **stderr** | `layout=` / `operbox=` 元数据；`profile JSON →` / `MAA 排班 JSON →` 路径提示 |
| **文件** | `--profile-out` 写账户画像 JSON；`--maa-out` 写 MAA 排班 JSON |

**前端结构化数据契约：**

前端不要解析 stdout 文本作为结构化数据。直接调用一次性 CLI 时，传入 `--profile-out` 和 `--maa-out` 两个路径，CLI 退出码为 0 后分别读取：

| 文件 | 用途 |
|------|------|
| `--profile-out` | 账户画像 JSON，见 §4.1 |
| `--maa-out` | MAA 排班 JSON：`title`、`description`、`plans[]`、`rooms` |

**示例：**

```bash
infra-cli plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/243_profile.json \
  --maa-out out/243_maa.json
```

### 4.1 用户画像 JSON（`--profile-out`）

`--profile-out` 当前稳定契约为 `schema_version: 4`。前端应把它作为结构化用户画像读取；stdout / stderr 只用于日志和人类可读报告。

```json
{
  "schema_version": 4,
  "rotation_profile": "abc_12_6_6",
  "layout_label": "data/fixtures/243/layout.json",
  "operbox_label": "data/fixtures/243/operbox_full_e2.json",
  "baseline_label": "data/fixtures/243/schedule_export.json (full_e2)",
  "summary": {
    "owned": 418,
    "tier_up_owned": 418,
    "trade_pool_ready": 75,
    "manufacture_pool_ready": 90
  },
  "domains": [
    {
      "id": "trade_gold",
      "label": "贸易·赤金线",
      "current": {
        "operators": ["巫恋", "龙舌兰", "卡夫卡"],
        "final_efficiency": 2.482,
        "mechanic_equivalent_efficiency": 0.460
      },
      "baseline": {
        "operators": ["巫恋", "龙舌兰", "卡夫卡"],
        "final_efficiency": 2.482,
        "mechanic_equivalent_efficiency": 0.460
      },
      "gap_ratio": 0.0,
      "severity": "ok"
    }
  ],
  "rotation": {
    "daily_trade_efficiency": 5.288,
    "daily_manufacture_efficiency": 9.175,
    "daily_power_efficiency": 3.552
  },
  "baseline_rotation": {
    "daily_trade_efficiency": 4.968,
    "daily_manufacture_efficiency": 8.951,
    "daily_power_efficiency": 3.552
  },
  "actions": [],
  "flags": ["trade_gold_ok"],
  "narration_hints": ["赤金贸易 meta 组合与公孙参考一致，差距主要在练度"]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `schema_version` | `4` | 直接效率契约版本；不兼容变更必须升版本 |
| `rotation_profile` | string | 本次画像与 `rotation` 指标使用的 profile：`abc_12_6_6`、`abc_12_12_12`、`main_backup_12_12`、`fiammetta_8_8_4_4`、`abyssal_7_5_7_5` |
| `layout_label` / `operbox_label` / `baseline_label` | `string` | 展示用来源标签，不作为路径依赖 |
| `summary` | object | 账号概览：`owned`、`tier_up_owned`、`trade_pool_ready`、`manufacture_pool_ready` |
| `domains[]` | array | 分域对比，UI 主表使用 |
| `domains[].id` | string | 稳定域 ID；已知值：`trade_gold`、`trade_originium`、`manufacture_total`、`manufacture_gold`、`manufacture_battle_record`、`rotation_trade`、`rotation_manufacture` |
| `domains[].current` / `baseline` | object | 当前 / 参考快照：`operators[]`、`final_efficiency`，贸易域可带 `mechanic_equivalent_efficiency` |
| `domains[].gap_ratio` | number | `(current.final_efficiency - baseline.final_efficiency) / baseline.final_efficiency` |
| `domains[].severity` | `"ok" | "warn" | "critical"` | 缺口等级 |
| `rotation` / `baseline_rotation` | object | 当前所选 profile 的日均指标 / 固定参考排班加权：`daily_trade_efficiency`、`daily_manufacture_efficiency`、`daily_power_efficiency`；`abc_12_12_12` 按 36h 周期归一化 |
| `actions[]` | array | 提升建议，元素含 `priority`、`kind`、`operator`、`domain_id`、`message` |
| `flags[]` | string[] | 机器可读状态标记 |
| `narration_hints[]` | string[] | 面向用户的补充说明 |

`rotation.shifts[].efficiencies.room_lines[]` 的统一效率字段由后端直接结算：
`operator_efficiency`（白字）、`station_efficiency`（绿字）、`total_efficiency`（倍率前总效率）、
`order_multiplier`、`final_efficiency`（倍率后实际效率）和 `equivalent_efficiency`（蓝字）。
前端不得根据旧的 `*_skill_efficiency` 或房间人数自行重算产出；排班和无人机执行结果必须消费后端结算值。
完整迁移步骤、缺省字段处理和验收清单见[前端效率字段迁移说明](前端效率字段迁移说明.md)。

### 4.2 `layout team-rotation`（仅排班 + MAA）

```bash
infra-cli layout team-rotation \
  --layout <蓝图.json> \
  --operbox <练度盒.json> \
  [--rotation <2 | 3 | 12/12/12 | fiammetta-8844 | abyssal-7575>] \
  --maa-out <输出/schedule.json>
```

**示例（仓库标准 243 夹具）：**

```bash
infra-cli layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

### 4.3 参数一览（team-rotation）

| 参数 | 必填 | 说明 |
|------|------|------|
| `--layout <path>` | 是 | `BaseBlueprint` JSON（见 §5.1） |
| `--operbox <path>` | 是 | `OperBox` JSON 数组（见 §5.2） |
| `--rotation <profile>` | 否 | 与 `plan` 相同；省略保持 ABC `12/6/6`；当前不接受自由时长数组 |
| `--maa-out <path>` | 否* | 写出 MAA 自定义基建 JSON；**前端集成建议始终带上** |
| `--maa-title <text>` | 否 | 覆盖 JSON 顶层 `title` |
| `--top <n>` | 否 | 编制搜索候选深度，默认 `20` |
| `--output-dir <dir>` | 否 | 额外写出每班 `BaseAssignment`（`team_shift_1.json` …） |
| `-o` / `--output <csv>` | 否 | 额外写 CSV 报告 |
| `--text` | 否 | 若未使用 `--maa-out`，人类可读排班走 stderr |
| `--json` | 否 | 内部 `TeamRotationReport` 走 stdout（调试） |

\* 带 `--maa-out` 时：`team-rotation` 的 **stderr = 人类可读排班表**；`plan` 的排班表在 **stdout**。

### 4.4 标准输出 / 标准错误

| 流 | `plan`（默认） | `team-rotation` + `--maa-out` |
|----|----------------|-------------------------------|
| **stdout** | 画像 + 排班表 | 空（除非 `-o` / `--json`） |
| **stderr** | 路径提示、元数据 | 人类可读排班表 + MAA 写入提示 |
| **文件** | `--profile-out` 账户画像 JSON；`--maa-out` MAA JSON | `--maa-out` |

**前端解析建议：**

- 一次性 CLI 集成：使用 **`plan`**，传入 `--profile-out` + `--maa-out`，读取对应文件。
- 常驻 Next BFF：使用 `infra-cli serve` 的 `plan.compute`，内联提交 layout/operbox，并从同一响应读取 profile、rotation、MAA 和结构化练卡建议。
- stdout / stderr 作为日志或人类可读报告展示，不作为结构化数据源。
- 下载 / 导入 MAA：一次性 CLI 使用 **`--maa-out` 已知路径** 读文件；常驻 Worker 直接读取 `result.maa`。
- 成功：`exit code == 0`；失败：stderr 含 `error:`，非零退出码。

`serve` 的机器主入口是 `method: "plan.compute"`：

下面只展示通过结构 schema 的最小形状，不承诺该单房布局可求解；真实调用必须内联提交完整 layout 和 operbox。

```json
{"id":1,"method":"plan.compute","params":{"schema_version":1,"layout":{"rooms":[{"id":"control","kind":"control_center","level":3}]},"operbox":[{"id":"char_009_12fce","name":"12F","elite":0,"level":30,"own":true,"potential":6,"rarity":2}],"labels":{"layout":"schema-example","operbox":"schema-example"},"options":{"rotation":"abc_12_6_6","top":20,"system_preferences":{},"maa_title":"My schedule"}}}
```

正常响应的 `result.schema_version` 等于请求参数的 schema 版本，并内联返回 `profile`（schema v4）、`rotation`（profile/daily/shifts 摘要）、`maa` 和 `training_advice`（schema v2）。`rotation.shifts[].assignment.rooms[].notes[]` 是 core notes owner 在最终班次编制上生成的房间级解释；前端按 `room_id` 展示 `text`，用 `code` 做稳定识别，不自行推断原因。v3 响应可选返回 `training_room`（schema v1）；该字段缺失时前端按“当前 Worker 未提供训练室结果”处理，不视为错误。`rotation.warnings`（报告级缺员提示，办公室/会客室候选不足时逐班列出 shift、设施、房间 ID 与缺额；无缺员时省略）与 MAA `hire`/`meeting` 槽位的 `autofill:true` 对应。练卡字段的完整契约见下一节。

`plan.compute` v1/v2/v3 参数的机器契约分别是 [`contracts/plan-compute-v1.schema.json`](../contracts/plan-compute-v1.schema.json)、[`contracts/plan-compute-v2.schema.json`](../contracts/plan-compute-v2.schema.json) 与 [`contracts/plan-compute-v3.schema.json`](../contracts/plan-compute-v3.schema.json)，人类可读契约见对应的 `plan-compute-v1.md` / `plan-compute-v2.md` / `plan-compute-v3.md`。Worker 在 Serde 反序列化前执行对应 Draft 2020-12 schema；params、layout、room、scenario、operbox、labels 和 options 均拒绝未知字段。v1 保持原语义，`abc_12_12_12` 通过 v2 请求使用，`options.fiammetta_enable`（默认 `true`）通过 v3 请求使用；`fiammetta_enable=false` 与 `rotation: fiammetta_8_8_4_4` 组合直接失败。

协议边界：request/response 单帧均不超过 8 MiB；layout 包含 1 至 64 个房间；operbox 包含 1 至 1000 项；`top` 为 1 至 100；layout/operbox label 均为非空且不超过 200 UTF-8 bytes。重复 room/operator ID、房间 kind 与 product 的跨字段一致性等不能由结构 schema 完整表达的规则，仍由 Rust 领域校验拒绝。

`ping` 的 result 返回 `protocol_version=1`、`plan_schema_version=1`、`plan_contract_sha256`、`solver_executable_sha256`、`solver_git_commit` 和 `solver_built_at`。前两个版本字段承担能力判定；所有 hash 和构建元数据都是诊断字段，不阻断 `plan.compute`。

每个 Worker 响应顶层都有 `solver: {git_commit, built_at}` 构建快照。前端必须从本次 `plan.compute` 响应保存 solver 快照，并将它写入对应反馈的 `meta.json`；不得使用最近一次 ping 或前端自身 revision。字段缺失时写 `solver: null`，不阻断反馈。

旧 `method: "plan"` 仅作为已发布前端的兼容入口，继续接收路径并按请求写 profile/MAA/shift 文件；它与 `plan.compute` 共用一次 Plan 编排，不是第二套 solver。新前端遇到 ping 缺少要求的版本字段时要求升级，不回退旧路径协议。

在 [集成与部署验收](TODO/Worker内联JSON集成与部署验收.md) 完成且部署 inventory 证明旧调用方已退出前，不得删除该兼容入口；退役工作单独由 [Worker 旧路径协议清理](TODO/Worker旧路径协议清理.md) 跟踪。

### 4.5 `training_advice`（排班附带的练卡建议）

**这是新前端唯一的练卡入口。** 调用 `plan.compute` 时没有额外的 advice 参数、文件路径或第二次 CLI 调用：后端对同一份内联 `operbox` 和 `layout` 完成排班后，自动将 `result.training_advice` 一并返回。独立 `infra-cli advice`（包括 `--player`）是 legacy 诊断入口，不是前端协议，前端不得解析其 stdout 或调用它补充本响应。

前端提交的输入仍是本节 `plan.compute` 请求中的 `operbox` 和 `layout`；两者均受对应请求 schema 校验。后端从 `layout` 派生以下 `context` 事实，并从实际 rotation 派生两个人均效率字段，调用方不能手工覆盖：

| `context` 字段 | 来源 | 用途 |
|---|---|---|
| `has_originium_shard_factory` | 是否有源石碎片制造站 | 源石碎片、泡泡/火神行动建议门闩 |
| `meeting_room_max_level` | 会客室最高等级 | 会客室条件说明 |
| `dormitory_level_sum` | 宿舍等级总和 | 宿舍条件说明 |
| `engineering_robot_count` | 除会客室外的设施等级和，上限 64 | 工程机器人条件说明 |
| `trade_average_efficiency_percent` | 每班每贸易站 `trade_skill_efficiency * 100 / operator_count` 的简单平均 | T9 新手章节跳过线 |
| `manufacturing_average_efficiency_percent` | 每班每制造站 `manufacture_skill_efficiency * 100 / operator_count` 的简单平均 | T9 新手章节跳过线 |

只有贸易均效严格大于 `30` 且制造均效严格大于 `25` 时，`newbie_section_status` 为 `skipped_by_efficiency`。缺任一均效或恰好等于阈值都不跳过。`context` 内未能确定的字段会省略，绝不以 `null` 表示。

#### 4.5.1 顶层 schema

`result.training_advice` 始终为以下对象，`schema_version` 必须为 `2`；不兼容变更会升该版本。所有下表中标为“可省略”的字段会被省略而非输出 `null`。

```json
{
  "schema_version": 2,
  "context": {
    "has_originium_shard_factory": true,
    "meeting_room_max_level": 3,
    "dormitory_level_sum": 20,
    "engineering_robot_count": 64,
    "trade_average_efficiency_percent": 31.25,
    "manufacturing_average_efficiency_percent": 26.0
  },
  "newbie_section_status": "shown",
  "incomplete_newbie": [],
  "combinations": [],
  "recommendations": []
}
```

| 字段 | 类型 | 前端消费规则 |
|---|---|---|
| `context` | object | 本次排班和布局已经确认的事实；只展示或解释，不自行重算或覆盖。 |
| `newbie_section_status` | `shown` \| `complete` \| `skipped_by_efficiency` | `shown` 才展示新手行动章节；其他值仍可使用 `incomplete_newbie` 做收集/完成度 UI。 |
| `incomplete_newbie` | `NewbieProgress[]` | 全量未完成基础名单，不受 10 人行动建议上限截断。 |
| `combinations` | `CombinationProgress[]` | 固定 17 组顺序的组合完成度；没有某组不是前端自行补齐的信号。 |
| `recommendations` | `PlayerRecommendation[]` | 已去重、稳定排序且最多 10 人的本轮行动清单；按数组顺序展示，不按名字或 `product` 重排。 |

共有嵌套类型：

| 类型 | JSON 形状 |
|---|---|
| `TrainingState` | `{ "elite": 0..2, "level"?: integer }` |
| `Target` | `{ "kind": "explicit" \| "no_requirement" \| "derive_from_skill_binding" \| "needs_review", "elite"?: integer, "level"?: integer }` |
| `Acquisition` | `{ "kind": "shop" \| "public_recruitment" \| "event" \| "redeem_code" \| "integrated_strategy", "detail": string }` |
| `Product` | `trade` \| `gold` \| `experience` \| `general_manufacturing` \| `originium_shards` |
| `Condition` | `{ "kind": "layout" \| "facility" \| "same_station" \| "dormitory" \| "engineering_robots" \| "custom", "key": string, "value"?: JSON value, "description": string }` |

#### 4.5.2 新手和组合完成度

`NewbieProgress`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `operator` | string | 干员中文名。 |
| `product` | `Product` | 基础名单所属产物桶。 |
| `action` | `acquire` \| `train` | 未拥有为 `acquire`，已拥有未达目标为 `train`。 |
| `current` | `TrainingState`，可省略 | 未拥有时省略。 |
| `target` | `Target` | 目标练度或待核对目标。 |
| `acquisition` | `Acquisition`，可省略 | 仅未拥有且知识已确认获取方式时存在。 |

`CombinationProgress`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` / `name` | string | 稳定组合 ID 与展示名称。 |
| `product` | `Product`，可省略 | 主要产物；感知信息等跨产物组可省略。 |
| `consumer_products` | `Product[]`，可省略 | 跨产物消费端；空数组省略。 |
| `tier` | `high_efficiency` \| `low_efficiency`，可省略 | 组合效率分层。 |
| `scale` | `small` \| `system` | 组合尺度。 |
| `facilities` | string[] | 目标设施。 |
| `state` | `complete` \| `needs_training` \| `missing_core` \| `missing_important` \| `needs_review` | 本组当前完成态。 |
| `completed_slots` / `total_slots` / `completion_percent` | integer | 已完成槽位、选定形态总槽位、向下取整完成百分比。 |
| `missing_core` / `untrained_core` / `missing_important` / `untrained_important` | string[]，可省略 | 为空时省略；用于组内缺口展示。 |
| `selected_alternative` | integer，可省略 | 多形态组合中被选中的 0-based 形态索引。 |
| `members` | `CombinationMemberProgress[]` | 选定形态的成员状态。 |

每个 `CombinationMemberProgress` 为 `{ operator, role, progress, owned, target_met, current?, target, counts_toward_completion }`：`role` 为 `core`、`important`、`secondary` 或 `hanger`；`progress` 为 `missing`、`owned_needs_training`、`ready` 或 `needs_review`。`current` 仅已拥有时存在。`counts_toward_completion` 说明该成员是否计入当前选定形态的完成槽位，前端不得用成员数自行替代 `completion_percent`。

#### 4.5.3 最多 10 人的行动建议

`PlayerRecommendation`：

| 字段 | 类型 | 说明 |
|---|---|---|
| `operator` | string | 干员中文名；全数组去重。 |
| `action` | `acquire` \| `train` | 本轮实际行动。 |
| `current` | `TrainingState`，可省略 | 未拥有时省略。 |
| `target` | `Target` | 建议目标。 |
| `priority` | 见下表 | 稳定类别，供视觉层级或筛选使用。 |
| `priority_rank` | integer | 后端排序键；数组已按该键和确定性次级规则排好，前端保持数组顺序。 |
| `reason` | `newbie_required` \| `combination_core` \| `combination_important` \| `standalone` | 本条最高优先级身份。 |
| `product` | `Product`，可省略 | 展示/筛选标签，不得据此重排。 |
| `combination_id` / `combination_name` | string，可省略 | 建议关联组合；散件可省略。 |
| `efficiency` | `{ value: number, unit: "order_percent" \| "production_percent", note?: string }`，可省略 | 已确认的知识事实，不是本次排班实测产出。 |
| `conditions` | `{ condition: Condition, status: "satisfied" \| "unsatisfied" \| "unknown" }[]`，可省略 | 条件事实；空数组省略。同站条件为说明，不应从建议中移除。 |
| `acquisition` | `Acquisition`，可省略 | 仅未拥有且已确认获取方式时存在。 |

`priority` 的可能值和从高到低的类别为：`newbie_four_star_elite_one`、`flagship_newbie`、`other_newbie`、`owned_tailor`、`automation_must_train`、`small_high_efficiency_core`、`small_high_efficiency_important`、`system_single_core_gap`、`other_important`、`lower_priority_core`、`high_efficiency_standalone`。前端只渲染后端给出的建议：无源石碎片制造站时源石碎片、泡泡和火神不在数组中；泡泡/火神均未达标且有源石线时两人作为原子对出现，名额不足两人时整对不出现。

#### 4.5.4 成功、失败与实现边界

- `plan.compute` 的非 assert 求解或结果阶段失败仍保持顶层 `ok: false`，并在返回 JSON 的 `result.error` 重复返回 `{ code: "PLAN_FAILED", stage: "plan.compute", message }`。若 core 已真实构造本次 rotation 报告，`result.rotation` 同时保留该报告（可能停在报告完成前的最后阶段）；profile/MAA/练卡字段只保留已经成功生成的部分。
- 若错误发生在 rotation 报告产生前，`result.rotation` 为 `null`，不得用空 assignment 或旧结果补齐；这表示当前输入没有可供返回的最终排班。
- `options.assert_invariants=true` 仍只返回顶层 `ok: false`、`error.code: "PLAN_FAILED"`、`error.stage: "plan.compute"`，不返回 `result`，以保持严格门禁 fail-fast。
- 前端必须先检查 `result.error` 或顶层 `error`：只有两者都缺失时才能消费 `maa`、`profile`、`training_advice` 或把 rotation 标记为可执行。
- 新手章节跳过不等于基础名单完成：`skipped_by_efficiency` 时仍保留全量 `incomplete_newbie`，只是其中条目不以新手理由占用 `recommendations` 的最多 10 人名额。
- `recommendations` 是行动待办；`combinations` 是全量组合状态。不要从前者反推后者，也不要把灰色组合缺口计入 10 人限制。
- 前端不得把旧 `training_recommendations.json`、legacy `advice` 的 `now`/`conditional` 输出或自然语言 `--answer` 混入本 schema。

### 4.6 `training_room`（训练室排班附带结果）

`training_room` 是 `plan.compute` 成功结果中的可选响应字段，不改变总体 `plan_schema_version: 3`，也不要求旧 Worker 升级。当前协议为：

```json
{
  "training_room": {
    "schema_version": 1,
    "shifts": [
      { "trainee": "干员名或 null", "trainer": "干员名或 null" }
    ]
  }
}
```

`shifts` 与 `result.rotation.shifts` 按位置对应。没有训练任务时 `trainee` 必须为 `null`；没有协助位时 `trainer` 为 `null`。字段缺失表示 Worker 尚未提供训练室结果，前端必须兼容读取，不得把缺失解释为训练室空配置或协议错误。实现任务见 [训练室输出与 plan.compute 协议](TODO/训练室输出与plan.compute协议.md)。

### 4.7 stderr 提示示例

```
MAA 排班 JSON 已写入: out/243_maa.json
  layout=data/fixtures/243/layout.json operbox=data/fixtures/243/operbox_full_e2.json owned=418
  导入 MAA：任务设置 → 基建换班 → 自定义模式 → 选择该 JSON（plan_index 从 0 起）
```

---

## 5. 输入 JSON 格式

### 5.1 布局 `--layout`（BaseBlueprint）

与 `tools/layout-gen` 导出、`data/layout/*.json` 同 schema。

```json
{
  "template": "243_2gold_trade",
  "drone_cap": 235,
  "scenario": {
    "sui_facility_count": 2,
    "dorm_occupant_count": 20,
    "initial_global": { "monster_cuisine": 3.0 }
  },
  "rooms": [
    { "id": "control", "kind": "control_center", "level": 3 },
    {
      "id": "trade_1",
      "kind": "trade_post",
      "level": 3,
      "product": { "trade": { "order": "gold" } }
    },
    {
      "id": "manu_3",
      "kind": "factory",
      "level": 3,
      "product": { "factory": { "recipe": "gold" } }
    },
    { "id": "dorm_1", "kind": "dormitory", "level": 3, "dorm_beds": 5, "dorm_ambience_level": 5 }
  ]
}
```

`kind` 取值：`control_center` | `trade_post` | `factory` | `power_plant` | `dormitory` | `office` | `meeting_room` | `workshop`。

贸易 `order`：`gold` | `originium` → MAA 导出为 `LMD` | `Orundum`。  
制造 `recipe`：`gold` | `battle_record` | `originium` → `Pure Gold` | `Battle Record` | `Originium Shard`。

生产站岗位数由 `level` 推导：贸易站 / 制造站一级 1 人、二级 2 人、三级 3 人。

宿舍字段：

- `dorm_beds`：宿舍容量，用于 MAA 宿舍补人。
- `dorm_ambience_level`：技能原文“每间宿舍每级”的有效等级，满宿舍一般为 5；旧布局未提供时 CLI 会兼容使用 `dorm_beds`，再退回 `level`。

参考样例：`release/fixtures/layout.json` 或 `data/fixtures/243/layout.json`。**推荐用 layout-gen 导出，字段与此一致。**

### 5.2 练度盒 `--operbox`（OperBox）

JSON **数组**，每项一名干员：

```json
[
  {
    "id": "char_009_12fce",
    "name": "12F",
    "elite": 0,
    "level": 30,
    "own": true,
    "potential": 6,
    "rarity": 2
  }
]
```

- `name`：须与游戏/MAA **中文名**一致（OCR 换班用）。
- `own: false` 的干员不参与排班。
- `rarity` 必须已知；`elite` 与 `level` 必须是该星级可达的真实练度，否则直接报错。
- 参考样例：`data/fixtures/243/operbox_full_e2.json`。

---

## 6. MAA 输出 JSON（`--maa-out`）

符合 [MAA 基建排班协议](https://docs.maa.plus/zh-cn/protocol/base-scheduling-schema.html)。

### 6.1 顶层结构

```json
{
  "title": "243_2gold_trade 基建排班",
  "description": "由 ArknightsInfraCalc 生成；…",
  "planTimes": "2班 | 3班 | 4班",
  "plans": [ /* 与所选 profile 的 2 / 3 / 4 个状态一一对应 */ ]
}
```

### 6.2 每个 plan

| 字段 | 说明 |
|------|------|
| `name` | 如 `Shift 1 · 12h · α+β` |
| `description` | 当前段时长与“多少小时后执行下一计划”的相对间隔说明；不是 MAA 自动定时器 |
| `Fiammetta` | 次数、时机和目标去重见 [菲亚梅塔使用规则](菲亚梅塔使用规则.md)；`fiammetta-8844` 的事件态由该 profile 定义 |
| `drones` | 默认赤金制造站无人机；`room`/`index`/`order` |
| `rooms` | 见下表 |

### 6.3 `rooms` 与内部 room_id 对应

| MAA 键 | 蓝图 room_id 规则 |
|--------|-------------------|
| `trading[]` | `trade_1`, `trade_2`, …（数组下标顺序） |
| `manufacture[]` | `manu_1`, `manu_2`, … |
| `power[]` | `power_1`, … |
| `dormitory[]` | `dorm_1`, …（休息队填入空床位） |
| `control[]` | 单元素，中枢 5 人 |
| `hire[]` | 各 `office_*` |
| `meeting[]` | `meeting` |
| `processing[]` | 加工站（若有） |

每个 slot 常见字段：`skip`, `product`, `operators`, `sort`, `autofill`。

`fiammetta-8844` 的事件态只将目标原工作房设为 `skip: false`，用于在换心情后送回原岗；其他房间、宿舍和无人机都跳过，避免把一次菲亚事件误执行成完整换班。

### 6.4 MAA 任务配置

集成协议见 [MAA Integration - Infrast](https://docs.maa.plus/zh-cn/protocol/integration.html)：

- `mode`: `10000`（Custom）
- `filename`: 指向 `--maa-out` 生成的 JSON
- `plan_index`: `0` 起，对应 `plans[0]`、`plans[1]` …

---

## 7. 其它子命令（可选）

| 命令 | 用途 |
|------|------|
| **`plan`** | **账号画像 + 所选定时换班 profile + MAA**（一次性 CLI 集成首选） |
| `layout team-rotation` | 仅排班；支持 2 / 默认 3 / 两个具名 4 班 profile |
| `layout test` | 单班贸易/制造 Top-K 搜索（无轮换） |
| `layout analyze` | 账号画像（不写 MAA） |
| `layout eval` | 给定 `--assignment` 结算直接效率 |

前端若使用一次性 CLI 子进程完成「练度导入 → 分析 → 排班 → 导出 MAA」，**用 `plan` 一条命令即可**；常驻 Next BFF 使用 §4.4 的 `serve` / `plan.compute`。

---

## 8. 前端集成示例（Node）

### 8.1 `plan`（一次性 CLI 集成）

```javascript
import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";

async function runPlan({ cliPath, repoRoot, operbox, layout, rotation, maaOut, profileOut }) {
  const args = ["plan", "--operbox", operbox];
  if (layout) args.push("--layout", layout);
  if (rotation) args.push("--rotation", rotation);
  args.push("--profile-out", profileOut);
  args.push("--maa-out", maaOut);

  const { stdout, stderr } = await new Promise((resolve, reject) => {
    let out = "", err = "";
    const child = spawn(cliPath, args, { cwd: repoRoot });
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", (d) => { err += d; });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0) reject(new Error(`infra-cli exit ${code}\n${err}`));
      else resolve({ stdout: out, stderr: err });
    });
  });

  const profileJson = JSON.parse(await readFile(profileOut, "utf8"));
  const maaJson = JSON.parse(await readFile(maaOut, "utf8"));
  return { profileJson, maaJson, reportText: stdout, logs: stderr };
}
```

排班 core 的 JSON 输出（`layout team-rotation --json`）另包含
`peak_mood_eta`：`eta_hours` 是最高效率 peak 班从满心情开始的最长工作时间，
`bottleneck` 是首个心情瓶颈，`per_op` 提供逐干员明细。每班直接效率位于
`shifts[].efficiencies`，时长折算后分别为 `weighted_trade`、
`weighted_manufacture`、`weighted_power`；日汇总位于 `daily`。MAA 文件本身仍只保留执行所需字段。

### 8.2 `layout team-rotation`（仅排班）
import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";

async function runTeamRotation({ cliPath, repoRoot, layout, operbox, rotation, maaOut, title }) {
  const args = [
    "layout", "team-rotation",
    "--layout", layout,
    "--operbox", operbox,
    "--maa-out", maaOut,
  ];
  if (rotation) args.push("--rotation", rotation);
  if (title) args.push("--maa-title", title);

  const stderr = await new Promise((resolve, reject) => {
    let err = "";
    const child = spawn(cliPath, args, { cwd: repoRoot });
    child.stderr.on("data", (d) => { err += d; });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0) reject(new Error(`infra-cli exit ${code}\n${err}`));
      else resolve(err);
    });
  });

  const maaJson = JSON.parse(await readFile(maaOut, "utf8"));
  return { scheduleText: stderr, maaJson };
}
```

---

## 9. 限制（告知产品 / UI）

- 干员名必须为**客户端语言**（国服中文）。
- `2` 当前采用完整状态干员互斥 policy；失败表示该受限策略无解，不是所有可能二班方案全局无解。
- `fiammetta-8844` 与 `abyssal-7575` 是具名 hard profiles；缺硬前提直接非零退出，不回退默认三班。五班和自由时长数组尚未开放。
- `abyssal-7575` 的 5h plan 会在 `dormitory[]` 中固定输出“单回宿管、群回宿管、歌蕾蒂娅”的同宿顺序；求解器在生成前验证床位与回复量。
- 心情终验覆盖完整循环中的连续工作段与 `0.5h` 余量，不证明宿舍恢复永续。MAA 的实际触发时间仍由任务配置或人工控制。
- 不建模完整心情曲线和宿管恢复。菲亚梅塔次数、时机和目标规则见 [菲亚梅塔使用规则](菲亚梅塔使用规则.md)；被换下者的具体宿舍操作由 MAA 自动处理，本项目不保证写入某个宿舍槽位。基础龙巫 reserve 已由 rotation 构造，但菲亚可改写最终房间成员。
- 会客室由求解器自动填满（每间两人）：优先有线索搜集速度技能的干员（按静态求值排序，搭档条件两人同房时生效），无可用技能干员时兜底任意空闲干员；会客室继续参与 ABC 轮换，办公室按定时换班规则逐班独立选择。仅当可占用干员不足时才保持空位并导出 `autofill: true`。
- `layout eval --assignment ...` 会在 JSON 的 `office` / `meeting` 字段返回显式编制的静态技能速度、心情修正、贡献明细以及 `ignored` / `unsupported`。该结果不代表线索概率模拟。
- 首次 243 最高合法练度 operbox 排班约 **5–15 秒**（CPU 搜索）；前端应加 loading。

---

## 10. 快速自测清单

```bash
# 0. 浏览器打开 release/layout-gen/index.html，导出 layout 或使用 fixtures

# 1. 一体化（推荐）
infra-cli plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/test_profile.json \
  --maa-out out/test_maa.json

# 2. 仅排班
infra-cli layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/test_maa.json

# 3. 确认账户画像：profile.summary.owned > 0 且 profile.domains.length > 0
# 4. 确认 MAA 班次数：默认 maa.plans.length === 3；--rotation 2 为 2；具名四班为 4
# 5. 确认贸易产物：maa.plans[0].rooms.trading[0].product === "LMD"
```

---

## 11. 联系 / 仓库

- Layout 生成器：`tools/layout-gen/index.html`（release 包内同文件）
- MAA 映射：`crates/infra-core/src/export/maa.rs`
- CLI 入口：`crates/infra-cli/src/commands/layout.rs`
