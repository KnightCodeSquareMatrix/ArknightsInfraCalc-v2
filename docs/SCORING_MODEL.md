# 搜索评分与排序策略

> 文档角色：canonical
> 生命周期状态：current
> 当前真源：`crates/infra-core/src/scoring/`; 各域 `search/*.rs`
> 代码范围：`crates/infra-core/src/scoring/{mod,components,metric}.rs`; `search/{trade,manufacture,power,control,joint_trade}.rs`; `layout/assign/pipeline.rs`
> 代码锚点：`scoring/components.rs::ScoringPolicyId`; `scoring/components.rs::evaluate_trade_manufacture_weighted_efficiency`; `search/trade.rs::trade_efficiency_sort_key`; `search/manufacture.rs::search_manufacture_triples`; `search/control.rs::evaluate_control_policy`; `layout/assign/pipeline.rs::dynamic_producer_candidates`
> 关联链路：[`模块/候选搜索与Bake.md`](模块/候选搜索与Bake.md)、[`模块/中枢与跨设施状态.md`](模块/中枢与跨设施状态.md)、[`调用链/跨设施状态刷新.md`](调用链/跨设施状态刷新.md)
> 核验基线：`947f1a2`

## 排序原则

生产域各自使用直接效率。贸易按 `TradeSearchHit::final_efficiency`，制造按 `ManuSearchHit::final_efficiency`，发电按 `PowerSearchHit::final_efficiency`。同域比较不会读取另一个域的分数，也不会把 shortcut 解释值重复加到产出。

制造多产线先分别搜索每种配方，再按 `ManuLineScenario` 线数加权；贸易多站先分别搜索赤金和源石订单，再由 `TradeStationScenario` 合并。`top_k` 只限制返回的报告行，不证明搜索只访问 K 个组合。

## 具名跨域 policy

`ScoringPolicyId::TradeManufactureWeightedEfficiencyV1` 是动态 producer 联合候选使用的显式 policy：比较已完整 resolve 的候选时，对贸易和制造效率分别乘权后相加。权重属于排序 policy，不改变任何单房结算。控制中枢使用 `ControlFillPolicy` 的顺序和 ledger 事实，不调用跨域总分。

候选闭包中的 `upper_bound`、prefix pruning 和 shadow branch 是剪枝或 policy；除非对应代码证明等价，文档和输出不得称为全局最优。完整池模式只改变候选范围，不改变域公式。

## tie-break 和失败

同分时使用稳定的名称/组合排序。中枢效果相同值只允许消费 `ControlEffectLedger` 记录的 `Winner`/`TiedWinner` 来源；被更高值覆盖的 `Suppressed` 来源不产生偏好。输入不合法、互斥冲突或 required anchor 缺失应返回错误/无解，不通过匿名分数悄悄降级。

## 核验

用固定 OperBox 记录 `combinations`、`evaluated`、排序键和 policy id，再与 `layout eval --explain-json` 的 assignment snapshot 对照。联合候选和剪枝变更需运行最终 assignment invariant gate；性能测量见 [`PERFORMANCE_ENGINEERING.md`](PERFORMANCE_ENGINEERING.md)。
