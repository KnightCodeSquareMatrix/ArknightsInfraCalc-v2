# 贸易单位产出锚

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：`data/trade_shortcuts.json` 的 `unit_output` 字段及其 source/precision
> 代码范围：`crates/infra-core/src/trade/{shortcut,solver,unit_output}.rs`; `data/trade_shortcuts.json`; `data/REGRESSION_CASES.csv`
> 代码锚点：`shortcut.rs::CommunityUnitOutputRule`; `shortcut.rs::validate_trade_shortcuts`; `solver.rs::resolve_unit_output`; `unit_output.rs::{compute_unit_output,daily_yield}`
> 关联链路：[`../模块/贸易求解.md`](../模块/贸易求解.md)、[`SHORTCUT_MATCHING.md`](SHORTCUT_MATCHING.md)
> 核验基线：`947f1a2`

特殊组合的社区单位产出是 L3 产出锚，不由等效效率反推，除非条目标记 `derived_from_rounded_equivalent`。每个改变单位产出的 shortcut 必须同时声明 source 和 precision；非有限、非正数、缺来源或重复 id 在加载时失败。

运行时由 `resolve_unit_output` 选择 regular/L3 unit，再由 `TradeProductionReport::daily_at_shift` 计算工作时长产出。`mechanic_equivalent_efficiency` 不参与第二次乘法。修改锚点时同步更新 JSON、来源注释、回归期望和 verify fixture。
