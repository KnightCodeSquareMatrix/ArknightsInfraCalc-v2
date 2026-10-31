# trade/interpreter 内部地图

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/模块/贸易求解.md
> 代码范围：`crates/infra-core/src/trade/{interpreter,solver,gold_flow,order_mechanic}.rs`; `crates/infra-core/src/types.rs`; `data/skill_table.json`
> 代码锚点：`crates/infra-core/src/trade/interpreter.rs::{TradeContext,collect_atoms,apply_trade_phases}`; `crates/infra-core/src/trade/interpreter.rs::{apply_atom,apply_eff_action,apply_limit_action,apply_peer_absorb}`; `crates/infra-core/src/trade/solver.rs::solve_trade_with_shift`
> 关联链路：`docs/模块/贸易求解.md`; `docs/INTERNAL/SHORTCUT_MATCHING.md`
> 核验基线：`947f1a2`

`apply_trade_phases` 从每名 runtime operator 的 buff id 收集 atom，按 `Phase::sort_key` 和 `phase_order` 排序，依次处理 condition、state、eff、limit、order tag、peer absorb 和 mood。跨过 Limit 阶段会重算 order limit；赤金订单的 `gold_flow::apply_gold_flow_chain` 在进入 PeerAbsorb 前或循环末执行一次；循环结束再做最终 limit recompute。

| Phase | 主要 owner | 写入 |
|---|---|---|
| StateWrite | `apply_state_write` | `state_pool` |
| Constant/PeerShare/EffVar/OrderVar/LimitVar | `apply_eff_action` | `settled_eff`, `variable_eff`, `direct_eff` |
| Limit | `apply_limit_action` / `apply_eff_action` | 上限增减 → 上限派生 settled 效率 → compression |
| OrderMechanic | `apply_order_mechanic` | order tags、law、LMD bonus |
| PeerAbsorb | `apply_peer_absorb` | peer 清零、owner settled eff |
| Mood | `apply_mood_action` | mood drain delta |

L1 不解析 L2 订单分布或 L3 shortcut；solver 在 `order_eff_pre` 之后调用对应模块。`GlobalInject` 在贸易 L1 不执行，control/global resolve 先把 manifest 注入 LayoutContext。

诊断顺序：buff ids/atoms → phase trace → limit/order tags → order mechanic → shortcut/unit output。新增 selector 要同时检查 `resolve_selector_value` 和 condition；不要在 CLI 补公式。

贸易 `Limit` 阶段内由 atom 的 `phase_order` 保证：上限增减≤40，冠军风采50，市井之道压缩60。冠军风采以非负 `limit_contrib` 合计换效率并写入 settled，孑能读取；招商引资继续在后续 `LimitVar` 读取压缩后的上限净增量。不得把两类上限派生效率混为一个后置阶段。
