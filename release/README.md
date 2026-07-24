# ArknightsInfraCalc beta release

**版本**：beta 2026-06-25  
**平台**：Windows x64 / Linux x86_64
**CLI 推荐入口**：`infra-cli plan`

本包面向 beta 用户、Linux 主机部署和前端联调。它包含 CLI、静态 Layout 生成器、243 样例输入和前端集成文档。

---

## 包内容

```
release/
├── infra-cli / infra-cli.exe
├── contracts/
│   ├── plan-compute-v1.schema.json
│   └── SHA256SUMS
├── data/                  # Linux 独立部署包会内置；Windows 旧包可放在 release 同级
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

运行 CLI 时需要能找到 `data/`。解析顺序：

1. `ARKNIGHTS_INFRA_DATA_DIR` 环境变量指定的目录。
2. 当前工作目录下的 `data/`。
3. CLI 可执行文件旁边的 `data/`。
4. CLI 可执行文件父目录旁边的 `data/`。
5. 开发构建时的仓库 `data/`。

Linux 服务部署推荐设置 `ARKNIGHTS_INFRA_DATA_DIR=/opt/arknights-infra/data`，这样 systemd / Docker 改变工作目录也不会影响运行。

Linux 发布包可在 Linux 构建机上生成：

```bash
bash scripts/build_release_linux.sh
```

默认输出到 `dist/arknights-infra-linux-x86_64/`。

---

## CLI 推荐入口

### 推荐：账号画像 + 排班 + MAA

用户主链路使用 `plan`。它一次性生成：

- 用户画像 JSON：`--profile-out`
- MAA 基建排班 JSON：`--maa-out`
- 人类可读分析与排班报告：stdout

```bash
./release/infra-cli plan \
  --layout release/fixtures/layout.json \
  --operbox release/fixtures/operbox_full_e2.json \
  --profile-out out/243_profile.json \
  --maa-out out/243_maa.json
```

Windows:

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

```bash
./release/infra-cli layout team-rotation \
  --layout release/fixtures/layout.json \
  --operbox release/fixtures/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

不要使用 `layout rotation`。它是废弃的 A-B-A 旧轮换入口。

---

## Layout 生成器

浏览器打开：

```bash
xdg-open release/layout-gen/index.html
```

Windows 可双击 `release\layout-gen\index.html`，或运行 `start release\layout-gen\index.html`。

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
