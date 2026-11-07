# 直接效率与整数结算

> 文档角色：canonical
> 生命周期状态：current
> 摘要：裁决千分整数效率、各生产域结算和排班输出字段的统一口径。
> 领域键：scoring.efficiency
> 当前真源：self
> 代码范围：`crates/infra-core/src/efficiency.rs`; `crates/infra-core/src/trade/solver.rs`; `crates/infra-core/src/search/trade.rs`; `crates/infra-core/src/manufacture/solver.rs`; `crates/infra-core/src/search/manufacture.rs`; `crates/infra-core/src/power/solver.rs`; `crates/infra-core/src/layout/assignment.rs`; `crates/infra-core/src/bake.rs`
> 代码锚点：`crates/infra-core/src/efficiency.rs::Efficiency`; `crates/infra-core/src/search/trade.rs::TradeEfficiencyBreakdown`; `crates/infra-core/src/search/manufacture.rs::ManuEfficiencyBreakdown`; `crates/infra-core/src/layout/assignment.rs::RoomEfficiencySnapshot`; `crates/infra-core/src/bake.rs::BAKE_SCHEMA_VERSION`
> 关联链路：`docs/调用链/技能到房间结算.md`; `docs/模块/贸易求解.md`; `docs/模块/制造求解.md`; `docs/模块/候选搜索与Bake.md`
> 核验基线：`947f1a2`

## 口径

`Efficiency` 是无量纲直接效率。内部为千分整数：`1000 == 1.000`，`1550 == 1.550`。技能百分数只在解释器边界转换为小数；CLI、JSON、CSV 和搜索排序不再公开匿名百分比字段。

贸易和制造逐房展示统一为：总效率 = 基础效率（设施基准 `1.000` + 每名非红脸、心情大于 0 的干员进驻增量 `0.010`）+ 干员技能效率 + 全局加成。`base_efficiency`、`operator_efficiency`、`global_efficiency` 分别承载这三项；`station_efficiency` 是兼容字段，混合进驻增量与全局加成，不参与再次求和。`equivalent_efficiency` 是已结算结果的等效解释值，不额外计入总效率或产出。

- `Add`、`Sum` 使用 checked `i32` 千分整数。
- `multiply` 在千分整数乘积处四舍五入；`scale_ratio` 在比例计算处四舍五入。
- 序列化输出小数（例如 `2.837`），Bake 行保存 `*_efficiency_millis`，加载后恢复 `Efficiency`。
- `Display` 固定三位小数；非法、非有限或越界输入失败。

## 各域边界

贸易：`TradeEfficiency` 同时保存纸面效率（基础效率 + 干员技能效率 + 全局加成）、订单机制等效项、单位产出和 `final_efficiency = total_efficiency × unit_output_multiplier`。搜索及排班逐房、周期效率均使用 `final_efficiency`；实际日产出沿原单位产出结算，不把该 final 再乘入产出。shortcut 的单位产出不能再把 `mechanic_equivalent_efficiency` 乘一次。

制造：单站总效率 = 基础效率 + 干员技能效率 + 全局加成；基础效率已包含非红脸干员的进驻增量。多线由 `ManuEfficiencyBreakdown::aggregate_lines` 按线数加权。仓库上限和产出分解与效率分开保存。

发电：`aggregate_power_efficiency` 汇总岗位增量；布局总发电基础值只注入一次。排班汇总保留贸易、制造、发电三个域，不能跨域相加成一个匿名分数。

## 排班与输出

`RoomEfficiencySnapshot` 是 assignment 上已结算的事实。出口层优先消费快照；只有手写 assignment 没有快照时才调用对应域 solver 重算。Trade 排班从完整快照的 `trade_total_efficiency × trade_unit_output_multiplier` 恢复 final（不依赖不序列化的 legacy `trade_final_efficiency`），否则由 Trade solver 重算；`line.total_efficiency` 仍是纸面值。`ShiftEfficiencies` 依据班次分钟数做整数比例折算，`DailyTotals` 分开报告各域。跨域组合排序及完整 Plan 对比仍须另有明确 policy / proof，不由本次周期汇总贯通推出。若需要组合排序，必须引用 [`SCORING_MODEL.md`](SCORING_MODEL.md) 中具名 policy。

## 诊断

看到小数差异，先确认输入的 `Efficiency::millis`、舍入位置和班次分钟；再检查域 breakdown；最后检查单位产出或全局注入。不要在 CLI 重新计算公式，也不要把 `mechanic_equivalent_efficiency` 当作第二次乘法。

验证入口：`infra-cli verify`、`layout eval --explain-json`、`plan --assert-invariants`、Bake 的 `validate`/`verify`。这些命令能证明给定输入和基线的行为，不能证明所有账号和未覆盖机制均等价。
