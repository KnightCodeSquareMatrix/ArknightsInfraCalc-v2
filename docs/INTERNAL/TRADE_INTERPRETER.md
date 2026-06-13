# trade/interpreter 内部地图

> 文件：`crates/infra-core/src/trade/interpreter.rs`（~1100 行，后半为 `mod tests`）。  
> 对外 API：`apply_trade_phases`、`TradeContext`、`OperatorRuntime`。

## 主循环 `apply_trade_phases`

1. `collect_atoms` — 按干员 `buff_ids` 从 `skill_table` 收集 `(EffectAtom, owner)`，按 `phase.sort_key()` + `phase_order` 排序。
2. 遍历 atom：
   - **赤金线挂钩**：首次进入 `PeerAbsorb` 及之后、且当前为赤金订单时，调用 `gold_flow::apply_gold_flow_chain`（只执行一次）。
   - **上限重算**：跨过 `Limit` 阶段后调用 `recompute_limit`。
   - `condition_met` 为 false 则跳过。
   - `apply_atom` 执行。
3. 循环结束若未跑过 gold_flow 且为赤金订单，再补一次 `apply_gold_flow_chain`。
4. 最终 `recompute_limit`。

**不要在这里改 L2 订单分布或 L3 shortcut** — 那些由 `solver.rs` 在 L1 结束后调用。

## Phase 分发表（`apply_atom`）

| Phase | 处理函数 | 主要副作用 |
|-------|----------|------------|
| `StateWrite` | `apply_state_write` | `state_pool` 写入 / `StateConvert` |
| `Constant` / `PeerShare` / `EffVar` / `OrderVar` / `LimitVar` | `apply_eff_action` | `operators[].settled_eff` / `variable_eff` / `direct_eff` |
| `Limit` | `apply_limit_action` | `limit_contrib`、`limit_compression` |
| `OrderMechanic` | `apply_order_mechanic` | `order_tags`、`breach_gold_add`、`order_lmd_bonus`、`law_active`；若 action 为 `AddFlatEff` 再走 `apply_eff_action` |
| `GlobalInject` | （空） | 贸易站 L1 不处理；中枢注入见 `control/interpreter.rs` |
| `PeerAbsorb` | `apply_peer_absorb` | 他人效率归零；主人 `settled_eff += peer_count × rate` |
| `Mood` | `apply_mood_action` | `operators[].mood_drain_delta` |

Phase 排序键见 `types.rs` 的 `Phase::sort_key`（`StateWrite` 10 → … → `Mood` 95）。

## 效率相关（最常改）

| 想改什么 | 函数 | 说明 |
|----------|------|------|
| 固定 +N% | `apply_eff_action` + `AddFlatEff` | `direct_eff` 仅无 selector 的 flat |
| 订单差 per-gap | `AddPerGapEff` | `OrderVar` 阶段 → `variable_eff` |
| 从 Selector 派生效率 | `resolve_eff_value` / `resolve_selector_value` | 新增 Selector 时两处都要补 |
| 状态池消费变效率 | `StateConsumeToEff` | 读 `state_pool` |
| 上限 +N | `apply_limit_action` → `AddLimitDelta` | `limit_contrib` |
| 孑式压缩上限 | `ReduceLimit` | `limit_compression` |
| 订单打 tag | `apply_order_mechanic` → `TagOrder` | L1 只打 tag；L2 在 `order_mechanic.rs` 解释 |

### `resolve_selector_value` 已实现的 Selector

`FinalOrderLimit`、`LimitExcess`、`FacilityLevel`、`TaggedCountInRoom`、`LimitContribSum`、`MeetingMaxLevel`、`DormLevelSum`、`ManuRecipeKinds`、`EliteFacilityCount`、`SuiFacilityCount`、`DormOccupantCount`、`OrderGap`、`OrderCount`、`PeerSettledEffSum`、`OtherOpsSettledEff`、`OtherOpsDirectEff`、`OtherOpsTotalEff`、`RoomPeerCount`、`RoomOperatorCount`、`Mood`、`TradeStationCount`、`PowerStationCount`（经 `layout.effective_power_station_count()`）、`GoldDeliveryCount`。

`TaggedCountInControl` / `ControlOperatorCount` 在贸易站上下文恒为 0（中控在 `control/` 单独求值后注入 layout）。

## Condition（`condition_met`）

| Condition | 判定要点 |
|-----------|----------|
| `GoldDeliveryBelow` / `Above` | 赤金订单 + `default_gold_delivery` |
| `GoldOrderInvestEligible` | 赤金、交付 >3、无 `breach` tag |
| `OrderHasTag` / `OrderNotHasTag` | `ctx.order_tags` |
| `MoodAbove` / `MoodBelowOrEq` | `ctx.mood` |
| `PartnerInRoom` | 同房干员名 |
| `TagPresentInRoom` | 干员 `tags` |
| `OperatorInBase` | `layout.base_workforce` |
| `TiandaoEffVarAllowed` | 市井之道 + 天道酬勤互斥规则（见 `tiandao_eff_var_allowed`） |
| `ActiveRecipe` | `ctx.active_order_kind` |
| `OperatorInPower` / `OwnerLacksBuff` | 当前恒 false（占位） |

## 上下文类型

| 类型 | 职责 |
|------|------|
| `TradeContext` | 同房运行时状态；`from_room` 从 `TradeRoomInput` 构建 |
| `OperatorRuntime` | 每人 `settled_eff` / `direct_eff` / `variable_eff` / `limit_contrib` / `mood_drain_delta` |
| `MechanicCaps` | 由 `ctx.mechanic_caps()` 导出给 L2 / `unit_output` |

效率汇总：`order_eff_base()`、`order_eff_skill()`、`order_eff_total()`（在 `TradeContext` impl 中，靠近文件前半）。

## 改机制时的文件顺序

1. `types.rs` — 新词汇
2. `data/skill_table.json`
3. 本文件对应函数段（多数情况无需通读 tests）
4. 若涉及 tag / 分布 → `order_mechanic.rs`；若组合表化 → `shortcut.rs` + `trade_shortcuts.json`
5. `solver.rs` 测试 / `REGRESSION_CASES.csv`

## 与制造站对称

`manufacture/interpreter.rs` 有相同的 `apply_*_phases` / `condition_met` / `resolve_selector_value` 结构，但上下文为 `ManuContext`（产能 / 仓库上限而非订单效率）。**不要假设贸易站 Phase 行为在制造站完全一致** — 改制造站前读 [MANUFACTURE_STATUS.md](../MANUFACTURE_STATUS.md)。
