# 243 标准测试样例

仓库默认的 layout + operbox 组合。**后续 Agent 无用户指定路径时，固定用本目录下 `layout.json` + `operbox_full_e2.json` 跑 `layout test`**（见 [AGENTS.md](../../AGENTS.md) §6.2、[INFRA_CLI.md](../../docs/INFRA_CLI.md)）。

| 文件 | 格式 | 说明 |
|------|------|------|
| `layout.json` | `BaseBlueprint` | 公孙 243 事实布局（2 金贸 + 2 经验制造 + 2 赤金制造）；同 `data/layout/243_use_this_.json` |
| `operbox_full_e2.json` | OperBox | 243 三班排班涉及干员，全部 **精2 / 90 级**；由 `schedule_export.json` + 种子 operbox 生成 |
| `schedule_export.json` | 一图流排班导出 | 243 高配 3 队 12H 换班；供 `scripts/build_243_schedule_fixture.py` 再生 assignment |

## 快速命令

```bash
cargo run -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text

cargo run -p infra-cli -- bench \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text

python scripts/build_243_schedule_fixture.py data/fixtures/243/schedule_export.json
```
