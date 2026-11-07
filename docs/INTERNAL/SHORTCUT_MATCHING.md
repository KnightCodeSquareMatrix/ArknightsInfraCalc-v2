# 贸易 shortcut 与 segment 匹配

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/模块/贸易求解.md
> 代码范围：`crates/infra-core/src/trade/{shortcut,segment,solver}.rs`; `crates/infra-core/src/search/role_pick.rs`; `data/trade_{shortcuts,segments}.json`
> 代码锚点：`crates/infra-core/src/trade/shortcut.rs::{load_trade_shortcuts,resolve_trade_shortcut,trade_station_exclusive_violation}`; `crates/infra-core/src/trade/segment.rs::{load_trade_segments,match_registered_trade_segment}`; `crates/infra-core/src/trade/solver.rs::solve_trade_with_shift_inner`; `crates/infra-core/src/search/role_pick.rs::pick_trade_role_hit`
> 关联链路：`docs/模块/贸易求解.md`; `docs/INTERNAL/TRADE_COMMUNITY_UNIT_OUTPUT.md`
> 核验基线：`947f1a2`
> 摘要：说明贸易 shortcut/segment 的真实匹配顺序、结算边界和诊断入口

求解顺序是：输入等级/互斥校验 → L1 phase → `order_eff_pre` → 赤金时尝试 registered segment 和 shortcut → 命中则构造 mechanic + community unit output → 未命中回 L2 `resolve_order_mechanic`。shortcut 只消费实际三人、tier、manifest 和 order context，不生成候选、不改变 Plan admission。

`trade_segments.json` 的 producer 条件只检查稳定 buff capability；`roles.pick_steps` 是 role fallback 顺序。`trade_station_exclusive_violation` 统一拒绝但书/巫恋侧/可露希尔/佩佩互斥，搜索与 solver 共享该事实。

条目中的 `mechanic_equivalent_efficiency` 是解释值；改变产出的条目必须有 `unit_output`、`unit_output_source` 和 `unit_output_precision`，加载验证失败即报错。新增 shortcut 同时补 fixture、regression case 和 verify 命令。
