# infra-cli 输入/输出格式完整参考

> 基于 `release/docs/FRONTEND_CLI.md` + `fixtures/` 样例 + `VERSION.txt` 整理
> 用途：调试页面开发前的格式摸底

---

## 一、命令总览

```
infra-cli.exe
├── plan                     ← 推荐：账号分析 + 排班 + MAA 一体
├── layout
│   ├── team-rotation        ← 仅 αβγ 三队排班 + MAA
│   ├── test                 ← 单班贸易/制造 Top-K 搜索（无轮换）
│   ├── analyze              ← 账号画像（不写 MAA）
│   └── eval                 ← 给定 --assignment 评估分数
```

---

## 二、输入格式

### 2.1 `--layout` → BaseBlueprint JSON

文件：`fixtures/layout.json` | 由 `layout-gen/index.html` 导出

```json
{
  "template": "243_2gold_trade",          // 模板名（字符串）
  "drone_cap": 135,                        // 无人机上限（数字）
  "scenario": {                            // 场景假设（可选字段）
    "sui_facility_count": 2,               // 岁设施数
    "dorm_occupant_count": 20,             // 宿舍人数
    "elite_facility_count": 0,             // 精英设施数（layout-gen 有，文档未列）
    "base_workforce": [],                  // 基本进驻编制（逗号分隔字符串数组）
    "initial_global": {
      "monster_cuisine": 0.0               // 魔物料理（浮点数）
    }
  },
  "rooms": [
    { "id": "control",   "kind": "control_center", "level": 3 },
    { "id": "trade_1",   "kind": "trade_post",      "level": 3,
      "product": { "trade": { "order": "gold" } } },
    { "id": "trade_2",   "kind": "trade_post",      "level": 3,
      "product": { "trade": { "order": "gold" } } },
    { "id": "manu_1",    "kind": "factory",         "level": 3,
      "product": { "factory": { "recipe": "battle_record" } } },
    { "id": "manu_2",    "kind": "factory",         "level": 3,
      "product": { "factory": { "recipe": "battle_record" } } },
    { "id": "manu_3",    "kind": "factory",         "level": 3,
      "product": { "factory": { "recipe": "gold" } } },
    { "id": "power_1",   "kind": "power_plant",     "level": 3 },
    { "id": "power_2",   "kind": "power_plant",     "level": 3 },
    { "id": "power_3",   "kind": "power_plant",     "level": 3 },
    { "id": "meeting",   "kind": "meeting_room",    "level": 3 },
    { "id": "office_1",  "kind": "office",          "level": 3 },
    { "id": "dorm_1",    "kind": "dormitory",       "level": 3, "dorm_beds": 5, "dorm_ambience_level": 5 },
    { "id": "dorm_2",    "kind": "dormitory",       "level": 3, "dorm_beds": 5, "dorm_ambience_level": 5 },
    { "id": "dorm_3",    "kind": "dormitory",       "level": 3, "dorm_beds": 5, "dorm_ambience_level": 5 },
    { "id": "dorm_4",    "kind": "dormitory",       "level": 3, "dorm_beds": 5, "dorm_ambience_level": 5 }
  ]
}
```

**`kind` 取值（8 种）：**

| kind 值 | 含义 |
|---------|------|
| `control_center` | 控制中枢 |
| `trade_post` | 贸易站 |
| `factory` | 制造站 |
| `power_plant` | 发电站 |
| `dormitory` | 宿舍 |
| `office` | 办公室（人力） |
| `meeting_room` | 会客室 |
| `workshop` | 加工站 |

**产物映射（trade / factory → MAA）：**

| 输入 `order` / `recipe` | MAA 产物 |
|------------------------|----------|
| `gold` → trade | `LMD` |
| `originium` → trade | `Orundum` |
| `gold` → factory | `Pure Gold` |
| `battle_record` → factory | `Battle Record` |
| `originium` → factory | `Originium Shard` |

---

### 2.2 `--operbox` → OperBox JSON / CSV / 一图流 xlsx

支持 **3 种输入格式**：

#### JSON 格式

文件：`fixtures/operbox_full_e2.json`（418 名干员，全精二 90 级）

顶级为 **JSON 数组**，每项一名干员：

```json
[
  {
    "id": "char_009_12fce",   // 游戏内部 ID
    "name": "12F",             // 中文名（必须与游戏/MAA 一致）
    "elite": 2,                // 精英化阶段 0/1/2
    "level": 90,               // 等级
    "own": true,               // 是否拥有（false 则不参与排班）
    "potential": 6,            // 潜能 1-6
    "rarity": 2                // 稀有度 1-6（星级）
  }
]
```

#### CSV 格式（一图流导出格式）

`--operbox` 直接接受 **一图流练度 CSV**（无需转 JSON/手动编辑）。列名为**中文**，第一行为表头：

```csv
序号,干员,精英化,等级,潜能,稀有度,拥有
char_009_12fce,12F,2,90,6,2,TRUE
char_113_cqbw,W,2,90,5,6,TRUE
```

| CSV 列 | 映射到 JSON 字段 | 说明 |
|--------|-----------------|------|
| 序号 | `id` | 游戏内部 ID（如 `char_009_12fce`） |
| 干员 | `name` | 中文名 |
| 精英化 | `elite` | 0/1/2 |
| 等级 | `level` | 等级数字 |
| 潜能 | `potential` | 1-6 |
| 稀有度 | `rarity` | 1-6（对应星级） |
| 拥有 | `own` | TRUE/FALSE（不区分大小写） |

> 注：该 CSV 格式可能与 JSON 的字段名/顺序不同，但 CLI 内部会统一解析为 OperBox。

#### XLSX 格式（一图流导出完整练度表）

`--operbox` 也接受**一图流导出的一体化 xlsx**。列数更多，包含技能专精和模组信息：

| XLSX 列名 | 样例 | 说明 |
|-----------|------|------|
| 干员名称 | 普罗旺斯 | 中文名 |
| 是否已招募 | true | 是否拥有 |
| 星级 | 5 | 1-6 |
| 等级 | 1 | 当前等级 |
| 精英化等级 | 0 | 0/1/2 |
| 潜能等级 | 1 | 1-6 |
| 通用技能等级 | 1 | 1-7 |
| 1技能专精等级 | 0 | 0-3 |
| 2技能专精等级 | 0 | 0-3 |
| 3技能专精等级 | 0 | 0-3 |
| χ分支模组 | 0 | 0-3 |
| γ分支模组 | 0 | 0-3 |
| Δ分支模组 | 0 | 0-3 |
| α分支模组 | 0 | 0-3 |

> 后端 CLI 内部仅提取 `干员名称` + `是否已招募` + `星级` + `等级` + `精英化等级` + `潜能等级` 用于排班，其余列（技能专精、模组）当前版本不参与排班逻辑。

实际文件参考：项目上级目录 `干员练度表.xlsx`（玩家练度，未拥有干员也保留在表中，`是否已招募=false`）。

> **注意**：该 xlsx 格式与前面提到的 CSV 格式是两套不同的一图流导出格式，CLI 均支持。CSV 是简化版（7列），xlsx 是完整版（14列）。

#### XLSX 格式（一图流导出）

`--operbox` 也接受 **一图流练度 xlsx** 直传，不需要转 JSON。CLI 内部会解析。

---

### 2.3 `--assignment` → BaseAssignment JSON（`layout eval` 用）

文档未给出完整 schema。用途是手动指定一个班次的分配方案，让 solver 评估分数。暂不展开。

---

## 三、输出格式

### 3.1 `plan` 命令的 stdout（人类可读）

- **stdout**：账号画像摘要 + αβγ 三队排班人类可读表
- **stderr**：元数据（`layout=` / `operbox=`）+ 路径提示（`MAA 排班 JSON 已写入: ...`）

stderr 示例：
```
MAA 排班 JSON 已写入: out/243_maa.json
  layout=data/fixtures/243/layout.json operbox=data/fixtures/243/operbox_full_e2.json owned=418
  导入 MAA：任务设置 → 基建换班 → 自定义模式 → 选择该 JSON（plan_index 从 0 起）
```

### 3.2 `team-rotation` 的 stderr（人类可读）

与 `plan` 的 stdout 格式相同，但走 stderr。前端解析建议同 `plan`。

### 3.3 `--maa-out` → MAA JSON（核心输出）

符合 [MAA 基建排班协议](https://docs.maa.plus/zh-cn/protocol/base-scheduling-schema.html)。

#### 顶层结构

```json
{
  "title": "243_2gold_trade 基建排班",
  "description": "由 ArknightsInfraCalc 生成；αβγ 三班轮换，12h+6h+6h",
  "plans": [ /* 3 个班次 */ ]
}
```

#### 每个 plan

| 字段 | 说明 |
|------|------|
| `name` | 如 `Shift 1 · 12h · α+β` |
| `description` | 班次说明文本 |
| `Fiammetta` | `{"enable": false, "target": [...]}` 菲亚梅塔技能（默认关） |
| `drones` | 无人机配置：`{"room": "manu_3", "index": 0, "order": 0, "mode": "efficiency"}` |
| `rooms` | 房间分配对象（见下） |

#### `rooms` 对象结构

| MAA 键 | 对应蓝图 ID | 说明 |
|--------|-------------|------|
| `trading[]` | `trade_1`, `trade_2`, ... | 贸易站数组，按下标对应 |
| `manufacture[]` | `manu_1`, `manu_2`, ... | 制造站数组 |
| `power[]` | `power_1`, ... | 发电站数组 |
| `dormitory[]` | `dorm_1`, ... | 宿舍（休息队填入空床位） |
| `control[]` | `control` | 单元素数组，中枢 5 人 |
| `hire[]` | `office_*` | 办公室/人力 |
| `meeting[]` | `meeting` | 会客室 |
| `processing[]` | `workshop` | 加工站 |

#### 每个 room slot 的常见字段

```json
{
  "product": "LMD",               // 产物（trade/manufacture 才有）
  "operators": ["阿米娅", "12F"],  // 干员名数组
  "sort": 0,                      // 排序（MAA 用）
  "autofill": true,               // 是否自动补满空位（dormitory/meeting）
  "skip": false                   // 是否跳过该房间
}
```

#### `product` 值映射（MAA 侧）：

| 输入 | MAA 输出值 |
|------|-----------|
| `gold` order | `LMD` |
| `originium` order | `Orundum` |
| `gold` recipe | `Pure Gold` |
| `battle_record` recipe | `Battle Record` |
| `originium` recipe | `Originium Shard` |

---

### 3.4 `--profile-out` → 账号画像 JSON

默认路径：`data/box_profile_<operbox名>.json`

文档未给出完整 schema。`plan` 命令默认会输出。`--json` 参数可将画像输出到 stdout（调试用）。

---

### 3.5 `-o` / `--output <csv>` → CSV 报告（`team-rotation`）

文档仅提到参数存在，未给出 CSV 列名和格式详情。需要实际运行查看。

---

### 3.6 `--output-dir <dir>` → 每班 team_shift JSON

文档提到会额外写出每班 `BaseAssignment`（`team_shift_1.json`, `team_shift_2.json`, ...）。

---

## 四、输出流总结

| 命令 | stdout | stderr | 文件输出 |
|------|--------|--------|----------|
| `plan`（默认） | 画像摘要 + 排班表 | 元数据 + 路径提示 | `--maa-out`, `--profile-out` |
| `plan --json` | 仅画像 JSON | 同上 | 同上 |
| `team-rotation` + `--maa-out` | 空 | 排班表 + 写入提示 | `--maa-out` |
| `team-rotation` + `--text` | 空 | 排班表 | （无） |
| `team-rotation` + `-o csv` | CSV | 同上 | `--maa-out` + CSV |
| `team-rotation --json` | JSON 报告 | 同上 | `--maa-out` |

---

## 五、调试页面需要支持的能力

```
┌─────────────────────────────────────────────────────────┐
│                    debug 页面需求                        │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ① 输入：layout JSON（上传 / 粘贴 / 默认 fixtures）      │
│  ② 输入：operbox JSON 或 xlsx（上传 / 粘贴）             │
│  ③ 调用：infra-cli plan（通过 Node.js spawn 或 HTTP API）│
│  ④ 输出：读取 --maa-out 的 MAA JSON → 渲染三区排班表      │
│  ⑤ 输出：展示 stdout（画像摘要）和 stderr（元数据）       │
│  ⑥ 辅助：展示产物映射（gold→LMD 等）是否正确              │
│  ⑦ 辅助：验证 plans.length === 3                        │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### MAA JSON 到 UI 的渲染映射

```
MAA rooms 键         UI 三区位置          房间色系
──────────────────────────────────────────────────
trading[]           左区 1-3 号         蓝色 trade_post
manufacture[]       左区 4-7 号         黄色 factory
power[]             左区 8-10 号        绿色 power_plant
control[]           中区 1 号           绿色 control_center
dormitory[]         中区 2-5 号         青色 dormitory
meeting[]           右区 2 号           灰色 meeting_room
processing[]        右区 3 号           灰色 workshop
hire[]              右区 4 号           灰色 office
```

---

## 六、当前缺失信息

| 项目 | 状态 |
|------|------|
| CSV 输出 (`-o`) 的列结构 | **待实测** |
| 画像 JSON schema (`--profile-out` / `--json`) | **待实测** |
| team_shift JSON schema (`--output-dir`) | **待实测** |
| 实际 MAA JSON 样例（有真实 operators 数组） | **待实测运行** |
