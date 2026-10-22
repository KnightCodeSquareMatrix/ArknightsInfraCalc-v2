# 搜索评分与排序策略

> 文档角色：canonical
> 生命周期状态：current
> 摘要：裁决各域排序、跨域具名 policy、剪枝保证等级和失败传播。
> 领域键：scoring.policy
> 当前真源：self
> 代码范围：`crates/infra-core/src/scoring/{mod,components,metric}.rs`; `crates/infra-core/src/search/{trade,manufacture,power,control,joint_trade}.rs`; `crates/infra-core/src/layout/assign/pipeline.rs`
> 代码锚点：`crates/infra-core/src/scoring/components.rs::ScoringPolicyId`; `crates/infra-core/src/scoring/components.rs::evaluate_trade_manufacture_weighted_efficiency`; `crates/infra-core/src/search/trade.rs::trade_efficiency_sort_key`; `crates/infra-core/src/search/manufacture.rs::search_manufacture_triples`; `crates/infra-core/src/search/control.rs::evaluate_control_policy`; `crates/infra-core/src/layout/assign/pipeline.rs::dynamic_producer_candidates`
> 关联链路：`docs/模块/候选搜索与Bake.md`; `docs/模块/中枢与跨设施状态.md`; `docs/调用链/跨设施状态刷新.md`
> 核验基线：`ddf5218`; `3f1d92e`; `01cbec5`; `target/codex-runs/trade-final-efficiency-unit-20261020-r2/`

## 排序原则

生产域各自使用直接效率。贸易按 `TradeSearchHit::final_efficiency`，制造按 `ManuSearchHit::final_efficiency`，发电按 `PowerSearchHit::final_efficiency`。贸易最终效率为纸面总效率乘特殊订单单位产出倍率；`mechanic_equivalent_efficiency` 只解释机制，不能再加一次。同域比较不会读取另一个域的分数。

制造多产线先分别搜索每种配方，再按 `ManuLineScenario` 线数加权；贸易 `TradeSearchOrderMode::Stations` 先分别搜索赤金和源石订单，再由 `TradeStationScenario` 按站数加权复合。Stations 的复合命中有 `breakdown=None`，不能把单房 comparator 或 `mechanic_equivalent_efficiency` 解释直接套到复合值。`top_k` 只限制返回的报告行，不证明搜索只访问 K 个组合。

## 具名跨域 policy

`ScoringPolicyId::TradeManufactureWeightedEfficiencyV1` 是动态 producer 联合候选使用的显式 policy：比较已完整 resolve 的候选时，对贸易和制造效率分别乘权后相加。该跨域 policy 仍读取纸面效率，尚未统一到单房 `final_efficiency`；这是开放项，不得当成已贯通。排班逐房及周期汇总已使用 Trade `final_efficiency`（纸面总效率乘单位产出倍率），不改变既有房间求和与分钟权重。权重属于排序 policy，不改变任何单房结算。控制中枢使用 `ControlFillPolicy` 的顺序和 ledger 事实，不调用跨域总分。

候选闭包中的 `upper_bound`、prefix pruning 和 shadow branch 是剪枝或 policy；除非对应代码证明等价，文档和输出不得称为全局最优。完整池模式只改变候选范围，不改变域公式。

## tie-break 和失败

同分时使用稳定的名称/组合排序。中枢效果相同值只允许消费 `ControlEffectLedger` 记录的 `Winner`/`TiedWinner` 来源；被更高值覆盖的 `Suppressed` 来源不产生偏好。输入不合法、互斥冲突或 required anchor 缺失应返回错误/无解，不通过匿名分数悄悄降级。

## 核验

用固定 OperBox 记录 `combinations`、`evaluated`、排序键和 policy id，再与 `layout eval --explain-json` 的 assignment snapshot 对照。单房 `final_efficiency` 反转可用算法 witness 证明比较公式，但不等于真实 CLI 已记录同房落选 trace；跨完整 Plan 比较需双方完整 assignment 与班时，现有单房排序不能外推。联合候选和剪枝变更需运行最终 assignment invariant gate；性能测量见 [`PERFORMANCE_ENGINEERING.md`](PERFORMANCE_ENGINEERING.md)。
