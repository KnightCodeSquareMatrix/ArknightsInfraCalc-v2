# 大文件内部地图

> 代码不拆分；用本文档族把千行级文件「切片」给 Agent / 开发者。**改前先查表，再打开对应函数段。**

| 文档 | 对应源码 | 何时读 |
|------|----------|--------|
| [TRADE_INTERPRETER.md](TRADE_INTERPRETER.md) | `crates/infra-core/src/trade/interpreter.rs` | 改 L1 Phase、Condition、Selector、效率/上限叠加 |
| [SHORTCUT_MATCHING.md](SHORTCUT_MATCHING.md) | `crates/infra-core/src/trade/shortcut.rs` | 改 L3 匹配、同房互斥、shortcut 回归 |
| [CROSS_FACILITY.md](CROSS_FACILITY.md) | `crates/infra-core/src/cross_facility/` | 改跨房 buff、scope=Global atom、resolve.rs 编排集成 |

**下一步**：编排层重构路线图见 [ORCHESTRATION_LAYER.md](../ORCHESTRATION_LAYER.md)（`layout/orchestrate/` 待建；System → Plan → Execute）。

制造站 L1 与贸易站对称，见 [MANUFACTURE_STATUS.md](../MANUFACTURE_STATUS.md)（入口 `manufacture/interpreter.rs`，结构类似 TRADE_INTERPRETER）。

CLI 输出层：`infra-cli/src/output.rs` 按 `emit_pool` / `emit_trade_search` / `emit_bench` / `emit_schedule` / `emit_trade_yield` / `emit_base_rotation` / `emit_team_rotation` 等函数名定位，无需通读全文。
