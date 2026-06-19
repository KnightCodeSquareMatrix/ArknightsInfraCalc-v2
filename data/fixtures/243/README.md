# 243 标准测试样例

仓库默认的 layout + operbox 组合。

- **用户说「跑一遍模拟」**：`layout team-rotation` + 本目录夹具 + `--maa-out out/243_maa.json`（见 [AGENTS.md](../../AGENTS.md) §6.2）
- **改机制 smoke test**：`layout test` + 本目录夹具（见 [AGENTS.md](../../AGENTS.md) §6.3）

| 文件 | 格式 | 说明 |
|------|------|------|
| `layout.json` | `BaseBlueprint` | 公孙 243 事实布局（2 金贸 + 2 经验制造 + 2 赤金制造）；同 `data/layout/243_use_this_.json` |
| `operbox_full_e2.json` | OperBox | 243 三班排班涉及干员，全部 **精2 / 90 级**；由 `schedule_export.json` + 种子 operbox 生成 |
| `schedule_export.json` | 一图流排班导出 | 243 高配 3 队 12H 换班；供 `scripts/build_243_schedule_fixture.py` 再生 assignment |

## 快速命令

```bash
# 默认模拟：αβγ 三队 ABC 轮换 + MAA JSON
cargo run -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json

# 改机制 smoke test：单班贸易/制造搜索
cargo run -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text

cargo run -p infra-cli -- bench \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text

python scripts/build_243_schedule_fixture.py data/fixtures/243/schedule_export.json
```
