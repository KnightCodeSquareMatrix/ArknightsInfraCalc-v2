# ArknightsInfraCalc — 前端对接 Release 包

**版本**：2026-06-18 · commit `482cf71` · Windows x64  
**完整说明**：[docs/FRONTEND_CLI.md](docs/FRONTEND_CLI.md)

---

## 包里有什么

```
release/
├── infra-cli.exe          ← 排班求解 + MAA JSON 导出（CLI）
├── layout-gen/
│   ├── index.html         ← 基建蓝图 Layout 生成器（静态页，浏览器打开）
│   └── README.md
├── fixtures/              ← 243 联调样例
│   ├── layout.json
│   └── operbox_full_e2.json
├── docs/
│   └── FRONTEND_CLI.md    ← 前端集成完整说明（参数、JSON、Node 示例）
├── README.md              ← 本文件
└── VERSION.txt
```

**还需要从仓库拷贝**（CLI 运行必需）：

```
data/                      ← operator_instances.json、skill_table.json、base_systems.json 等
```

---

## 端到端流程（Layout 页 → 排班 → MAA）

```
① layout-gen/index.html     用户点选 243 预设、改房间产物
        ↓ 导出 JSON
② my_layout.json            BaseBlueprint
        +
   operbox.json             玩家练度（xlsx 亦可，见 plan 命令）
        ↓ infra-cli plan 或 layout team-rotation --maa-out
③ stdout / stderr           账号分析 + 人类可读排班表
④ schedule.json             MAA 自定义基建 JSON（给用户下载 / 导入 MAA）
```

---

## 推荐命令（一体化）

在**仓库根目录**（须能访问 `./data/`）：

```powershell
.\release\infra-cli.exe plan `
  --operbox release\fixtures\operbox_full_e2.json `
  --maa-out out\243_maa.json
```

- 默认布局：`data/fixtures/243/layout.json`
- 输出：账号画像 JSON + αβγ 三队排班表（stdout）+ MAA JSON（`--maa-out`）
- `--operbox` 支持 **一图流练度 xlsx** 直传

仅要排班、不要账号分析时：

```powershell
.\release\infra-cli.exe layout team-rotation `
  --layout release\fixtures\layout.json `
  --operbox release\fixtures\operbox_full_e2.json `
  --maa-out out\243_maa.json
```

| 输出 | 位置 |
|------|------|
| 账号分析 + 排班表 | **stdout**（`plan`）或 **stderr**（`team-rotation`） |
| MAA JSON | `--maa-out` 指定路径 |
| 画像 JSON | `plan` 默认写 `data/box_profile_<operbox名>.json`，可用 `--profile-out` 覆盖 |

---

## ① Layout 生成器

```powershell
start release\layout-gen\index.html
```

- 选 **243** 等预设 → 点房间改等级/产物 → **导出 JSON**
- 也可 **导入 JSON** 编辑已有 layout

详见 [layout-gen/README.md](layout-gen/README.md)。

---

## CLI 最少参数

### `plan`（推荐）

```text
infra-cli plan
  --operbox  <练度盒.json | 一图流.xlsx>   [必填]
  [--layout  <蓝图.json>]                  [默认 243 fixtures]
  [--maa-out <MAA schedule.json>]
  [--profile-out <画像.json>]
  [--maa-title "标题"]
```

### `layout team-rotation`（仅排班）

```text
infra-cli layout team-rotation
  --layout   <Layout 生成器导出的 JSON>
  --operbox  <练度盒 JSON>
  --maa-out  <MAA schedule.json>
  [--maa-title "标题"]
```

- 成功：`exit code 0`
- 失败：非 0 + stderr 错误信息

---

## 打包发给前端（建议 Zip 结构）

```
ArknightsInfraCalc-frontend-release/
├── infra-cli.exe
├── layout-gen/
│   └── index.html
├── fixtures/
│   ├── layout.json
│   └── operbox_full_e2.json
├── data/                    ← 从仓库复制整个 data/
├── docs/
│   └── FRONTEND_CLI.md
├── README.md
└── VERSION.txt
```

解压后 **cwd 设为包根目录**（与 `data/` 同级），再运行 CLI。

---

## MAA 导入

1. 使用 `--maa-out` 生成的 JSON  
2. MAA → 任务设置 → 基建换班 → **自定义模式**（mode `10000`）  
3. 协议：https://docs.maa.plus/zh-cn/protocol/base-scheduling-schema.html  

---

## 其它平台

```bash
cargo build --release -p infra-cli
# 产物：target/release/infra-cli（或 infra-cli.exe）
# layout-gen 仍为静态 HTML，跨平台通用
```

详细 API、JSON 字段、Node 调用示例 → **[docs/FRONTEND_CLI.md](docs/FRONTEND_CLI.md)**
