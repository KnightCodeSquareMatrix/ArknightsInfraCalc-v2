# ArknightsInfraCalc — 前端对接 Release 包

**版本**：release 构建 · commit `1a37932` · Windows x64  
**完整说明**：[docs/FRONTEND_CLI.md](../docs/FRONTEND_CLI.md)

---

## 包里有什么

```
release/
├── infra-cli.exe          ← 排班求解 + MAA JSON 导出（CLI）
├── layout-gen/
│   ├── index.html         ← 基建蓝图 Layout 生成器（静态页，浏览器打开）
│   └── README.md
├── fixtures/              ← 联调样例（可选）
│   ├── layout.json
│   └── operbox_full_e2.json
├── README.md              ← 本文件
└── VERSION.txt
```

**还需要从仓库拷贝**（CLI 运行必需）：

```
data/                      ← operator_instances.json、skill_table.json 等
docs/FRONTEND_CLI.md
```

---

## 端到端流程（Layout 页 → 排班 → MAA）

```
① layout-gen/index.html     用户点选 243 预设、改房间产物
        ↓ 导出 JSON
② my_layout.json            BaseBlueprint
        +
   operbox.json             玩家练度（或 fixtures/operbox_full_e2.json）
        ↓ infra-cli layout team-rotation --maa-out
③ stderr                    人类可读排班表（给 UI 展示）
④ schedule.json             MAA 自定义基建 JSON（给用户下载 / 导入 MAA）
```

---

## ① Layout 生成器

用浏览器打开：

```
release/layout-gen/index.html
```

或：

```powershell
start release\layout-gen\index.html
```

- 选 **243** 等预设 → 点房间改等级/产物 → **导出 JSON**
- 也可 **导入 JSON** 编辑已有 `data/fixtures/243/layout.json`

详见 [layout-gen/README.md](layout-gen/README.md)。

---

## ②③④ 排班 + MAA 导出

在**仓库根目录**（或任意含 `data/` 的目录）：

```powershell
.\release\infra-cli.exe layout team-rotation `
  --layout release\fixtures\layout.json `
  --operbox release\fixtures\operbox_full_e2.json `
  --maa-out out\243_maa.json
```

若 layout 来自生成器，把 `--layout` 换成你导出的文件路径即可。

| 输出 | 位置 |
|------|------|
| 人类可读排班表 | **stderr** |
| MAA JSON | `--maa-out` 指定路径 |
| 写入提示 | stderr 末尾 `MAA 排班 JSON 已写入: ...` |

---

## CLI 最少参数

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

---

## MAA 导入

1. 使用 `--maa-out` 生成的 JSON  
2. MAA → 任务设置 → 基建换班 → **自定义模式**（mode `10000`）  
3. 协议：https://docs.maa.plus/zh-cn/protocol/base-scheduling-schema.html  

---

## 其它平台

```bash
cargo build --release -p infra-cli
# 产物：target/release/infra-cli
# layout-gen 仍为静态 HTML，跨平台通用
```

详细 API、JSON 字段、Node 调用示例 → **[docs/FRONTEND_CLI.md](../docs/FRONTEND_CLI.md)**
