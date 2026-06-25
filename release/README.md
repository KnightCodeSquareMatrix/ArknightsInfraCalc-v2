# ArknightsInfraCalc beta release

**版本**：beta 2026-06-25  
**平台**：Windows x64  
**CLI 推荐入口**：`infra-cli.exe plan`

本包面向 beta 用户和前端联调。它包含 Windows CLI、静态 Layout 生成器、243 样例输入和前端集成文档。

---

## 包内容

```
release/
├── infra-cli.exe
├── layout-gen/
│   ├── index.html
│   └── README.md
├── fixtures/
│   ├── layout.json
│   └── operbox_full_e2.json
├── docs/
│   └── FRONTEND_CLI.md
├── plans/
│   └── cli-format-reference.md
├── README.md
└── VERSION.txt
```

运行 CLI 时，工作目录需要能找到 `data/`。最简单做法是在仓库根目录运行；如果单独分发，请把整个 `data/` 目录和 `release/` 放在同一根目录下。

---

## CLI 推荐入口

### 推荐：账号画像 + 排班 + MAA

用户主链路使用 `plan`。它一次性生成：

- 用户画像 JSON：`--profile-out`
- MAA 基建排班 JSON：`--maa-out`
- 人类可读分析与排班报告：stdout

```powershell
.\release\infra-cli.exe plan `
  --layout release\fixtures\layout.json `
  --operbox release\fixtures\operbox_full_e2.json `
  --profile-out out\243_profile.json `
  --maa-out out\243_maa.json
```

前端不要解析 stdout / stderr 作为结构化数据；成功后读取 `--profile-out` 和 `--maa-out` 两个 JSON 文件。

### 仅排班：不需要用户画像时

只有在明确不需要账号画像时，才使用 `layout team-rotation`。

```powershell
.\release\infra-cli.exe layout team-rotation `
  --layout release\fixtures\layout.json `
  --operbox release\fixtures\operbox_full_e2.json `
  --maa-out out\243_maa.json
```

不要使用 `layout rotation`。它是废弃的 A-B-A 旧轮换入口。

---

## Layout 生成器

浏览器打开：

```powershell
start release\layout-gen\index.html
```

在页面中选择 243 / 153 / 333 / 252 / 342 等布局，导出 `BaseBlueprint` JSON，然后作为 `plan --layout` 输入。

---

## MAA 导入

1. 用 `--maa-out` 生成 JSON。
2. MAA → 任务设置 → 基建换班 → 自定义模式。
3. 选择生成的 JSON；`plan_index` 从 0 开始，对应三个班次。

---

## 文档

- 完整前端/CLI 契约：`release/docs/FRONTEND_CLI.md`
- CLI 输出参考：`release/plans/cli-format-reference.md`
