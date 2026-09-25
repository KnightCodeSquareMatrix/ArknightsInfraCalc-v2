# trade/interpreter 内部地图

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：`crates/infra-core/src/trade/interpreter.rs`、`types.rs`
> 代码范围：`crates/infra-core/src/trade/{interpreter,solver,gold_flow,order_mechanic}.rs`; `types.rs`; `data/skill_table.json`
> 代码锚点：`trade/interpreter.rs::{TradeContext,collect_atoms,apply_trade_phases}`; `trade/interpreter.rs::{apply_atom,apply_eff_action,apply_limit_action,apply_peer_absorb}`; `trade/solver.rs::solve_trade_with_shift`
> 关联链路：[`../模块/贸易求解.md`](../模块/贸易求解.md)、[`SHORTCUT_MATCHING.md`](SHORTCUT_MATCHING.md)
> 核验基线：`947f1a2`

`apply_trade_phases` 从每名 runtime operator 的 buff id 收集 atom，按 `Phase::sort_key` 和 `phase_order` 排序，依次处理 condition、state、eff、limit、order tag、peer absorb 和 mood。跨过 Limit 阶段会重算 order limit；赤金订单的 `gold_flow::apply_gold_flow_chain` 在 PeerAbsorb 后或循环末执行一次；循环结束再做最终 limit recompute。

| Phase | 主要 owner | 写入 |
|---|---|---|
| StateWrite | `apply_state_write` | `state_pool` |
| Constant/PeerShare/EffVar/OrderVar/LimitVar | `apply_eff_action` | `settled_eff`, `variable_eff`, `direct_eff` |
| Limit | `apply_limit_action` | `limit_contrib`, compression |
| OrderMechanic | `apply_order_mechanic` | order tags、law、LMD bonus |
| PeerAbsorb | `apply_peer_absorb` | peer 清零、owner settled eff |
| Mood | `apply_mood_action` | mood drain delta |

L1 不解析 L2 订单分布或 L3 shortcut；solver 在 `order_eff_pre` 之后调用对应模块。`GlobalInject` 在贸易 L1 不执行，control/global resolve 先把 manifest 注入 LayoutContext。

诊断顺序：buff ids/atoms → phase trace → limit/order tags → order mechanic → shortcut/unit output。新增 selector 要同时检查 `resolve_selector_value` 和 condition；不要在 CLI 补公式。
