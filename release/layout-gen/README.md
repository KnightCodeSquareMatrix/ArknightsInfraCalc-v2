# 基建 Layout 生成器

单文件静态页，**无需构建**。双击或在浏览器打开 `index.html` 即可。

## 功能

- 243 / 153 / 333 等基建预设（贸/制/电数量）
- 点击房间编辑等级、贸易订单、制造配方、宿舍床位数
- 场景假设：无人机上限、岁设施数、宿舍人数、魔物料理等
- **导出 / 复制 / 导入** `BaseBlueprint` JSON

## 与 infra-cli 衔接

1. 在本页配置布局 → **导出 JSON**（或复制后存为 `my_layout.json`）
2. 在仓库根目录（含 `data/`）运行：

```powershell
..\infra-cli.exe layout team-rotation `
  --layout my_layout.json `
  --operbox ..\fixtures\operbox_full_e2.json `
  --maa-out ..\out\schedule.json
```

导出 JSON 即 CLI 的 `--layout` 输入；格式与 `data/fixtures/243/layout.json` 相同。

源码同步位置：`tools/layout-gen/index.html`（与 release 包内文件一致）。
