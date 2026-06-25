# infra-cli + Layout 生成器 — 前端对接说明

> 面向前端 / 排班 UI。**Layout 蓝图**用静态页 `layout-gen` 编辑；**排班求解 + MAA JSON** 用 `infra-cli`（子进程或后续 WASM）。  
> **Beta Release 构建**：2026-06-25 · backend commit `9e52de9` · frontend beta source `3259eaa` · 见 `release/VERSION.txt`

---

## 1. Release 包内容

```
release/
├── infra-cli.exe              Windows x64 CLI（约 3 MB）
├── layout-gen/
│   └── index.html             基建 Layout 生成器（浏览器打开，无构建）
├── fixtures/
│   ├── layout.json            243 样例蓝图
│   └── operbox_full_e2.json   243 样例练度（全精2）
├── docs/
│   └── FRONTEND_CLI.md        本文件副本
├── plans/
│   └── cli-format-reference.md
├── README.md                  快速上手
└── VERSION.txt
```

另需从仓库拷贝 **`data/`**（CLI 机制数据，必需）。

Linux / macOS CLI：

```bash
cargo build --release -p infra-cli
# layout-gen/index.html 跨平台通用，无需编译
```

---

## 2. Layout 生成器（`layout-gen`）

### 2.1 打开方式

```powershell
# 资源管理器双击，或：
start release/layout-gen/index.html
```

源码与 release 包同步：`tools/layout-gen/index.html`（单文件，内联 CSS/JS）。

### 2.2 能力

| 功能 | 说明 |
|------|------|
| 基建预设 | 243 / 153 / 333 / 252 / 342（贸/制/电数量） |
| 房间编辑 | 等级、贸易订单（LMD/合成玉）、制造配方、宿舍床位 |
| 场景假设 | `drone_cap`、`sui_facility_count`、`dorm_occupant_count`、`monster_cuisine` 等 |
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

## 3. 运行前提：data 目录

`layout team-rotation` 会加载机制数据，路径解析顺序：

1. 当前工作目录下的 `./data/operator_instances.json`、`./data/skill_table.json`（若存在）
2. 否则使用**编译时**仓库内的 `data/`（开发机从 repo 根目录运行即可）

**发给前端测试时，请一并提供整个 `data/` 目录**，或保证进程 `cwd` 下存在 `data/`。

最小必需文件：

- `data/operator_instances.json`
- `data/skill_table.json`
- （间接）`data/base_systems.json` 等布局体系锚点

---

## 4. 主命令

### 4.0 `plan`（推荐：账号分析 + 排班一体）

```bash
infra-cli plan \
  --operbox <练度盒.json | 一图流.xlsx> \
  [--layout <蓝图.json>] \
  [--maa-out <输出/schedule.json>] \
  [--profile-out <画像.json>]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `--operbox <path>` | 是 | `OperBox` JSON 数组，或一图流导出的 **xlsx** |
| `--layout <path>` | 否 | 默认 `data/fixtures/243/layout.json` |
| `--maa-out <path>` | 否* | MAA 自定义基建排班 JSON；前端主链路应传入已知临时路径 |
| `--profile-out <path>` | 否 | 账号画像 JSON；前端主链路应传入已知临时路径 |
| `--baseline <operbox>` | 否 | 对比基准练度盒（画像用） |
| `--maa-title <text>` | 否 | 覆盖 MAA JSON 顶层 `title` |
| `--top <n>` | 否 | 搜索深度，默认 `20` |
| `--output-dir <dir>` | 否 | 额外写出每班 `team_shift_*.json` |
| `--json` | 否 | 仅输出账户画像 JSON 到 stdout（调试）；不包含 MAA JSON |

**输出约定（默认，无 `--json`）：**

| 流 | 内容 |
|----|------|
| **stdout** | 账号画像摘要 + αβγ 三队排班人类可读表 |
| **stderr** | `layout=` / `operbox=` 元数据；`profile JSON →` / `MAA 排班 JSON →` 路径提示 |
| **文件** | `--profile-out` 写账户画像 JSON；`--maa-out` 写 MAA 排班 JSON |

**前端结构化数据契约：**

前端不要解析 stdout 文本作为结构化数据。主链路应始终传入 `--profile-out` 和 `--maa-out` 两个路径，CLI 退出码为 0 后分别读取：

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

`--profile-out` 当前稳定契约为 `schema_version: 2`。前端应把它作为结构化用户画像读取；stdout / stderr 只用于日志和人类可读报告。

```json
{
  "schema_version": 2,
  "layout_label": "data/fixtures/243/layout.json",
  "operbox_label": "data/fixtures/243/operbox_full_e2.json",
  "baseline_label": "data/fixtures/243/schedule_export.json (full_e2)",
  "summary": {
    "owned": 418,
    "tier_up_owned": 418,
    "trade_pool_ready": 75,
    "manu_pool_ready": 90
  },
  "domains": [
    {
      "id": "trade_gold",
      "label": "贸易·赤金线",
      "current": {
        "operators": ["巫恋", "龙舌兰", "卡夫卡"],
        "score": 3.4748,
        "trade_pct": 138.0,
        "gold_pct": 46.0
      },
      "baseline": {
        "operators": ["巫恋", "龙舌兰", "卡夫卡"],
        "score": 3.4748,
        "trade_pct": 138.0,
        "gold_pct": 46.0
      },
      "gap_ratio": 0.0,
      "severity": "ok"
    }
  ],
  "rotation": {
    "daily_trade": 6.48335,
    "daily_manu": 482.625,
    "daily_power": 51.375
  },
  "baseline_rotation": {
    "daily_trade": 5.84553,
    "daily_manu": 465.75,
    "daily_power": 55.125
  },
  "actions": [],
  "flags": ["trade_gold_ok"],
  "narration_hints": ["赤金贸易 meta 组合与公孙参考一致，差距主要在练度"]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `schema_version` | `2` | 用户画像契约版本；不兼容变更必须升版本 |
| `layout_label` / `operbox_label` / `baseline_label` | `string` | 展示用来源标签，不作为路径依赖 |
| `summary` | object | 账号概览：`owned`、`tier_up_owned`、`trade_pool_ready`、`manu_pool_ready` |
| `domains[]` | array | 分域对比，UI 主表使用 |
| `domains[].id` | string | 稳定域 ID；已知值：`trade_gold`、`trade_originium`、`manu_composite`、`manu_gold`、`manu_exp`、`rotation_trade`、`rotation_manu` |
| `domains[].current` / `baseline` | object | 当前 / 参考快照：`operators[]`、`score`，贸易域可带 `trade_pct` / `gold_pct` |
| `domains[].gap_ratio` | number | `(current.score - baseline.score) / baseline.score` |
| `domains[].severity` | `"ok" | "warn" | "critical"` | 缺口等级 |
| `rotation` / `baseline_rotation` | object | 24h 三队轮休加权：`daily_trade`、`daily_manu`、`daily_power` |
| `actions[]` | array | 提升建议，元素含 `priority`、`kind`、`operator`、`domain_id`、`message` |
| `flags[]` | string[] | 机器可读状态标记 |
| `narration_hints[]` | string[] | 面向用户的补充说明 |

### 4.2 `layout team-rotation`（仅排班 + MAA）

```bash
infra-cli layout team-rotation \
  --layout <蓝图.json> \
  --operbox <练度盒.json> \
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

- 一体化 UI：优先 **`plan`**，传入 `--profile-out` + `--maa-out`，读取 `profileJson` / `maaJson`。
- stdout / stderr 作为日志或人类可读报告展示，不作为结构化数据源。
- 下载 / 导入 MAA：使用 **`--maa-out` 已知路径** 读文件。
- 成功：`exit code == 0`；失败：stderr 含 `error:`，非零退出码。

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
  "drone_cap": 135,
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
    }
  ]
}
```

`kind` 取值：`control_center` | `trade_post` | `factory` | `power_plant` | `dormitory` | `office` | `meeting_room` | `workshop`。

贸易 `order`：`gold` | `originium` → MAA 导出为 `LMD` | `Orundum`。  
制造 `recipe`：`gold` | `battle_record` | `originium` → `Pure Gold` | `Battle Record` | `Originium Shard`。

参考样例：`release/fixtures/layout.json` 或 `data/fixtures/243/layout.json`。**推荐用 layout-gen 导出，字段与此一致。**

### 5.2 练度盒 `--operbox`（OperBox）

JSON **数组**，每项一名干员：

```json
[
  {
    "id": "char_009_12fce",
    "name": "12F",
    "elite": 2,
    "level": 90,
    "own": true,
    "potential": 6,
    "rarity": 2
  }
]
```

- `name`：须与游戏/MAA **中文名**一致（OCR 换班用）。
- `own: false` 的干员不参与排班。
- 参考样例：`data/fixtures/243/operbox_full_e2.json`。

---

## 6. MAA 输出 JSON（`--maa-out`）

符合 [MAA 基建排班协议](https://docs.maa.plus/zh-cn/protocol/base-scheduling-schema.html)。

### 6.1 顶层结构

```json
{
  "title": "243_2gold_trade 基建排班",
  "description": "由 ArknightsInfraCalc 生成；…",
  "plans": [ /* 3 个班次，αβγ 为 12h + 6h + 6h */ ]
}
```

### 6.2 每个 plan

| 字段 | 说明 |
|------|------|
| `name` | 如 `Shift 1 · 12h · α+β` |
| `description` | 班次说明 |
| `Fiammetta` | 默认 `enable: false`；`target` 可按班次预填龙舌兰/但书 |
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

### 6.4 MAA 任务配置

集成协议见 [MAA Integration - Infrast](https://docs.maa.plus/zh-cn/protocol/integration.html)：

- `mode`: `10000`（Custom）
- `filename`: 指向 `--maa-out` 生成的 JSON
- `plan_index`: `0` 起，对应 `plans[0]`、`plans[1]` …

---

## 7. 其它子命令（可选）

| 命令 | 用途 |
|------|------|
| **`plan`** | **账号画像 + αβγ 排班 + MAA**（前端首选） |
| `layout team-rotation` | 仅 αβγ 三队排班 + MAA |
| `layout test` | 单班贸易/制造 Top-K 搜索（无轮换） |
| `layout analyze` | 账号画像（不写 MAA） |
| `layout eval` | 给定 `--assignment` 评估分数 |
| **`layout rotation`** | **已废弃**（A-B-A）；请用 `team-rotation` 或 `plan` |

前端若做「练度导入 → 分析 → 排班 → 导出 MAA」，**用 `plan` 一条命令即可**。

---

## 8. 前端集成示例（Node）

### 8.1 `plan`（推荐）

```javascript
import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";

async function runPlan({ cliPath, repoRoot, operbox, layout, maaOut, profileOut }) {
  const args = ["plan", "--operbox", operbox];
  if (layout) args.push("--layout", layout);
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

### 8.2 `layout team-rotation`（仅排班）
import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";

async function runTeamRotation({ cliPath, repoRoot, layout, operbox, maaOut, title }) {
  const args = [
    "layout", "team-rotation",
    "--layout", layout,
    "--operbox", operbox,
    "--maa-out", maaOut,
  ];
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
- 不建模心情曲线、宿管、菲亚梅塔自动放技能（菲亚梅塔字段默认关闭，可后续 UI 编辑 JSON）。
- 会客室若 solver 未分配人，导出为 `autofill: true`。
- 首次 243 全精2 operbox 排班约 **5–15 秒**（CPU 搜索）；前端应加 loading。

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
# 4. 确认 MAA 三班：maa.plans.length === 3
# 5. 确认贸易产物：maa.plans[0].rooms.trading[0].product === "LMD"
```

---

## 11. 联系 / 仓库

- Layout 生成器：`tools/layout-gen/index.html`（release 包内同文件）
- MAA 映射：`crates/infra-core/src/export/maa.rs`
- CLI 入口：`crates/infra-cli/src/commands/layout.rs`
