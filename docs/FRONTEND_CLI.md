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
  [--rotation <2 | 3 | fiammetta-8844 | abyssal-7575>] \
  [--maa-out <输出/schedule.json>] \
  [--profile-out <画像.json>]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `--operbox <path>` | 是 | `OperBox` JSON 数组，或一图流导出的 **xlsx** |
| `--layout <path>` | 否 | 默认 `data/fixtures/243/layout.json` |
| `--rotation <profile>` | 否 | 默认 `3`；可选 `2`、`fiammetta-8844`、`abyssal-7575`；裸 `4` 硬错误 |
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
| `rotation_profile` | string | 本次画像与 `rotation` 指标使用的 profile：`abc_12_6_6`、`main_backup_12_12`、`fiammetta_8_8_4_4`、`abyssal_7_5_7_5` |
| `layout_label` / `operbox_label` / `baseline_label` | `string` | 展示用来源标签，不作为路径依赖 |
| `summary` | object | 账号概览：`owned`、`tier_up_owned`、`trade_pool_ready`、`manufacture_pool_ready` |
| `domains[]` | array | 分域对比，UI 主表使用 |
| `domains[].id` | string | 稳定域 ID；已知值：`trade_gold`、`trade_originium`、`manufacture_total`、`manufacture_gold`、`manufacture_battle_record`、`rotation_trade`、`rotation_manufacture` |
| `domains[].current` / `baseline` | object | 当前 / 参考快照：`operators[]`、`final_efficiency`，贸易域可带 `mechanic_equivalent_efficiency` |
| `domains[].gap_ratio` | number | `(current.final_efficiency - baseline.final_efficiency) / baseline.final_efficiency` |
| `domains[].severity` | `"ok" | "warn" | "critical"` | 缺口等级 |
| `rotation` / `baseline_rotation` | object | 当前所选 24h profile / 固定参考排班加权：`daily_trade_efficiency`、`daily_manufacture_efficiency`、`daily_power_efficiency` |
| `actions[]` | array | 提升建议，元素含 `priority`、`kind`、`operator`、`domain_id`、`message` |
| `flags[]` | string[] | 机器可读状态标记 |
| `narration_hints[]` | string[] | 面向用户的补充说明 |

### 4.2 `layout team-rotation`（仅排班 + MAA）

```bash
infra-cli layout team-rotation \
  --layout <蓝图.json> \
  --operbox <练度盒.json> \
  [--rotation <2 | 3 | fiammetta-8844 | abyssal-7575>] \
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
| `--rotation <profile>` | 否 | 与 `plan` 相同；省略保持 ABC `12/6/6` |
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

响应 `result.schema_version=1`，并内联返回 `profile`（schema v4）、`rotation`（profile/daily/shifts 摘要）、`maa` 和 `training_advice`（schema v2）。`training_advice` 固定包含新手章节状态 `newbie_section_status`、全量 `incomplete_newbie`、常见组合 `combinations` 和最多 10 人的 `recommendations`；建议可附带 `product`、`efficiency` 和逐条件 `conditions`。它消费本次请求的 operbox 与 layout，已知无源石碎片制造站时不会推荐搓玉干员或泡泡/火神，且泡泡组有源石线时成对输出；同站条件只作说明。排班结果按每班每站 `skill-efficiency ÷ operator_count` 的简单平均注入 T9 上下文，超过贸易 30% 且制造 25% 时跳过新手行动建议但保留完整未完成清单。`rotation.shifts[]` 的 `efficiencies.room_lines[]` 还包含 `operator_count`，便于前端复核均效口径。

`plan.compute` v1 参数的机器契约是 [`contracts/plan-compute-v1.schema.json`](../contracts/plan-compute-v1.schema.json)，人类可读契约见 [`contracts/plan-compute-v1.md`](../contracts/plan-compute-v1.md)。Worker 在 Serde 反序列化前执行该 Draft 2020-12 schema；params、layout、room、scenario、operbox、labels 和 options 均拒绝未知字段。v1 发布后不可原地增加、删除或改变请求字段，任何结构或语义扩展必须使用新版本。

协议边界：request/response 单帧均不超过 8 MiB；layout 包含 1 至 64 个房间；operbox 包含 1 至 1000 项；`top` 为 1 至 100；layout/operbox label 均为非空且不超过 200 UTF-8 bytes。重复 room/operator ID、房间 kind 与 product 的跨字段一致性等不能由结构 schema 完整表达的规则，仍由 Rust 领域校验拒绝。

`ping` 的 result 返回 `protocol_version=1`、`plan_schema_version=1` 和 `plan_contract_sha256`。hash 是 Worker 内嵌 schema 原始 bytes 的 SHA-256，可与发布包 `contracts/SHA256SUMS` 对账；它不替代 schema version。

旧 `method: "plan"` 仅作为已发布前端的兼容入口，继续接收路径并按请求写 profile/MAA/shift 文件；它与 `plan.compute` 共用一次 Plan 编排，不是第二套 solver。新前端遇到 ping 缺少要求的版本字段或契约 hash 时要求升级，不回退旧路径协议。

在 [集成与部署验收](TODO/Worker内联JSON集成与部署验收.md) 完成且部署 inventory 证明旧调用方已退出前，不得删除该兼容入口；退役工作单独由 [Worker 旧路径协议清理](TODO/Worker旧路径协议清理.md) 跟踪。

### 4.5 stderr 提示示例

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
| `Fiammetta` | 默认 ABC 可有一次休息班回岗；`fiammetta-8844` 的第二个 8h plan 是菲亚事件态 |
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
- 不建模完整心情曲线和宿管恢复。当前每个 ABC 周期只安排一次菲亚主力回岗；被换下者的具体宿舍操作由 MAA 自动处理，本项目不保证写入某个宿舍槽位。基础龙巫 reserve 已由 rotation 构造，但菲亚可改写最终房间成员。布局动态排序和跨周期就绪模拟尚未实现，详见 [Fiammetta.md](Fiammetta.md)。
- 会客室若 solver 未分配人，导出为 `autofill: true`。
- `layout eval --assignment ...` 会在 JSON 的 `office` / `meeting` 字段返回显式编制的静态技能速度、心情修正、贡献明细以及 `ignored` / `unsupported`。该结果不代表自动选人或线索概率模拟。
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
