# 候选搜索与 Bake

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md
> 摘要：定位候选池、Plan 门禁、域搜索、排序与 Bake 回退的完整链路。
> 代码范围：`crates/infra-core/src/{pool,search,bake.rs,candidate.rs}`; `crates/infra-cli/src/commands/{bake,profile}.rs`; `data/baked/`
> 代码锚点：`pool/base.rs::{build_roster_pool,filter_pool}`; `search/{trade,manufacture,control,power,joint_trade}.rs`; `bake.rs::{bake_catalogs,validate_baked_catalog,try_baked_trade_search}`
> 关联链路：[`调用链/技能到房间结算.md`](../调用链/技能到房间结算.md)、[`调用链/跨设施状态刷新.md`](../调用链/跨设施状态刷新.md)
> 核验基线：`947f1a2`

## 搜索链

```text
OperBox + runtime data -> facility pool -> Plan/exclusion/used/capacity
 -> domain search -> comparator/policy -> hit/report
 -> assignment commit -> resolve -> next stage
```

pool/admission、容量、required anchor、互斥和输入匹配是 hard constraint。已证明等价的枚举才可称 safe reduction；top-k、排序、control policy、producer prefix 和 shadow branch 都是 policy/heuristic。完整池模式不能被 standalone 或 Bake 子集替代。

## 域入口

`search_trade_triples`、`search_manufacture_triples`、`search_control_combos`、`search_power_assignment` 各自负责域内比较；`joint_trade` 只处理具名跨域 candidate policy。遇到“候选没被考虑”，按 pool skip → Plan claim/exclusion → used → capacity → domain filter → comparator 排查。

## Bake

`bake_catalogs` 用 live solver 生成 schema v13 catalog 和 input fingerprint。`validate_baked_catalog` 检查 schema、输入 hash、row 集与 combo table hash。`BakeMode::Auto` 不兼容就回 live，`Disabled` 禁用，`Required` 对 miss 失败。生成物不可手工编辑为规则。

核验：`bake validate`、`bake verify`、`infra-cli verify`，并记录输入、组合数、evaluated 和命中模式。候选/剪枝/Bake 改动还要运行最终 assignment invariant gate。
