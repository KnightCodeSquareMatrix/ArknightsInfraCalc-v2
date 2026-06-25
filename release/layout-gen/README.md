# Layout 生成器

`index.html` 是单文件静态页，无需构建。双击或用浏览器打开即可。

## 用法

1. 选择 243 / 153 / 333 / 252 / 342 等预设。
2. 编辑房间等级、贸易订单、制造配方、宿舍床位、宿舍有效等级和场景假设。
3. 导出 `BaseBlueprint` JSON。
4. 把导出的 JSON 传给 `infra-cli.exe plan --layout`。

```powershell
..\infra-cli.exe plan `
  --layout my_layout.json `
  --operbox ..\fixtures\operbox_full_e2.json `
  --profile-out ..\out\profile.json `
  --maa-out ..\out\schedule.json
```

仅排班、不需要用户画像时，才改用 `layout team-rotation`。
