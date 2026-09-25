# 直接效率与整数结算

> 文档角色：canonical
> 生命周期状态：current
> 当前真源：`crates/infra-core/src/efficiency.rs`、各域 `*SearchHit`/`*Result`；游戏机制见 [`知识库引用.md`](知识库引用.md)
> 代码范围：`crates/infra-core/src/efficiency.rs`; `trade/solver.rs`; `search/trade.rs`; `manufacture/solver.rs`; `search/manufacture.rs`; `power/solver.rs`; `layout/assignment.rs`; `bake.rs`
> 代码锚点：`efficiency.rs::Efficiency`; `search/trade.rs::TradeEfficiencyBreakdown`; `search/manufacture.rs::ManuEfficiencyBreakdown`; `layout/assignment.rs::RoomEfficiencySnapshot`; `bake.rs::BAKE_SCHEMA_VERSION`
> 关联链路：[`调用链/技能到房间结算.md`](调用链/技能到房间结算.md)、[`模块/贸易求解.md`](模块/贸易求解.md)、[`模块/制造求解.md`](模块/制造求解.md)、[`模块/候选搜索与Bake.md`](模块/候选搜索与Bake.md)
> 核验基线：`947f1a2`

## 口径

`Efficiency` 是无量纲直接效率。内部为千分整数：`1000 == 1.000`，`1550 == 1.550`。技能百分数只在解释器边界转换为小数；CLI、JSON、CSV 和搜索排序不再公开匿名百分比字段。

- `Add`、`Sum` 使用 checked `i32` 千分整数。
- `multiply` 在千分整数乘积处四舍五入；`scale_ratio` 在比例计算处四舍五入。
- 序列化输出小数（例如 `2.837`），Bake 行保存 `*_efficiency_millis`，加载后恢复 `Efficiency`。
- `Display` 固定三位小数；非法、非有限或越界输入失败。

## 各域边界

贸易：`TradeEfficiency` 同时保存纸面效率、订单机制等效项、单位产出和 `final_efficiency`。搜索主键是 `TradeSearchHit::final_efficiency`，实际日产出再乘 `unit_trade_per_day × shift_hours / 24`；shortcut 的单位产出不能再把 `mechanic_equivalent_efficiency` 乘一次。

制造：单站 `final_efficiency = base + occupancy + skill + global`；多线由 `ManuEfficiencyBreakdown::aggregate_lines` 按线数加权。仓库上限和产出分解与效率分开保存。

发电：`aggregate_power_efficiency` 汇总岗位增量；布局总发电基础值只注入一次。排班汇总保留贸易、制造、发电三个域，不能跨域相加成一个匿名分数。

## 排班与输出

`RoomEfficiencySnapshot` 是 assignment 上已结算的事实。出口层优先消费快照；只有手写 assignment 没有快照时才调用对应域 solver 重算。`ShiftEfficiencies` 依据班次分钟数做整数比例折算，`DailyTotals` 分开报告各域。若需要组合排序，必须引用 [`SCORING_MODEL.md`](SCORING_MODEL.md) 中具名 policy。

## 诊断

看到小数差异，先确认输入的 `Efficiency::millis`、舍入位置和班次分钟；再检查域 breakdown；最后检查单位产出或全局注入。不要在 CLI 重新计算公式，也不要把 `mechanic_equivalent_efficiency` 当作第二次乘法。

验证入口：`infra-cli verify`、`layout eval --explain-json`、`plan --assert-invariants`、Bake 的 `validate`/`verify`。这些命令能证明给定输入和基线的行为，不能证明所有账号和未覆盖机制均等价。
