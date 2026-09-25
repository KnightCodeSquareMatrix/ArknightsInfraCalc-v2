# 搜索性能与 Bake 边界

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：`crates/infra-core/src/bake.rs`、`pool/`、`search/`；策略分类见 [`SCORING_MODEL.md`](SCORING_MODEL.md)
> 代码范围：`crates/infra-core/src/{bake.rs,pool,search,candidate.rs,profile.rs}`; `crates/infra-cli/src/commands/{bake,profile}.rs`; `data/baked/`
> 代码锚点：`bake.rs::{bake_catalogs,validate_baked_catalog,try_baked_trade_search}`; `pool/base.rs::{build_roster_pool,filter_pool}`; `search/trade.rs::search_trade_triples`; `search/manufacture.rs::search_manufacture_triples`; `layout/assign/pipeline.rs::{run_shift_pipeline,run_shift_pipeline_with_producer_pruning}`
> 关联链路：[`模块/候选搜索与Bake.md`](模块/候选搜索与Bake.md)、[`模块/中枢与跨设施状态.md`](模块/中枢与跨设施状态.md)、[`SCORING_MODEL.md`](SCORING_MODEL.md)
> 核验基线：`947f1a2`

## 成本模型

主要成本是对候选组合运行真实 domain solver，并在部分 assignment、producer prefix 和 shadow branch 上重复 resolve。组合数由 pool size 和人数决定；报告的 `combinations` 与 `evaluated` 必须保留，不能只报 wall time。

## 可接受的优化

- `Pool` 过滤、房间容量、required admission、`used`、互斥和配方/订单是 hard constraint。
- 已证明等价的域内枚举可写为 safe reduction；top-k、稳定排序、control policy、producer prefix pruning 和 shadow branch 属于 policy/heuristic。
- 任何剪枝若不能证明等价，只能声称在该 policy 下搜索，不得声称全局最优。
- `full_pool` 不能被 standalone pool 或 Bake 子集静默替换；动态 context 必须重新检查。

## Bake 生命周期

`BAKE_SCHEMA_VERSION` 当前为 13。`bake_catalogs` 以同一 live solver 生成 catalog、manifest、operator/signature 和 combo table；manifest 保存输入文件 fingerprint、生成选项、schema 和 row hash。`validate_baked_catalog` 在 schema、fingerprint、row 集或 hash 不匹配时拒绝。

运行时 `BakeMode::Auto` 命中兼容行，否则回退 live；`Disabled` 永不读取；`Required` 对任何 miss/error 失败。Bake 生成期间禁止自读生成物。`bake verify` 用 live solver 对 signature 的首/中/尾响应抽样对账；生成物不能手工改成语义来源。

## 诊断与验证

性能报告至少记录 commit、输入 OperBox、pool size、组合数、evaluated、命中模式和 stage timing。正确性用 `bake validate`、`bake verify`、`infra-cli verify` 和最终 assignment gate；性能数据不能证明机制覆盖，Bake 校验也不能证明未抽样行的全局等价。
