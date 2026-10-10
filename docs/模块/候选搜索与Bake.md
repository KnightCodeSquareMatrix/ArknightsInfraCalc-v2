# 候选搜索与 Bake

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/SCORING_MODEL.md; docs/PERFORMANCE_ENGINEERING.md
> 机制依赖：docs/知识库引用.md；vault:docs/1-基础设定/设施与进驻/基建总览.md；vault:docs/3-组合库/组合注册表.md
> 摘要：定位候选池、Plan 门禁、域搜索、排序与 Bake 回退的完整链路。
> 代码范围：`crates/infra-core/src/{pool,search,bake.rs,candidate.rs}`; `crates/infra-cli/src/commands/{bake,profile}.rs`; `data/baked/`
> 代码锚点：`crates/infra-core/src/pool/base.rs::{build_roster_pool,filter_pool}`; `crates/infra-core/src/search/{trade,manufacture,control,power,joint_trade}.rs`; `crates/infra-core/src/bake.rs::{bake_catalogs,validate_baked_catalog,try_baked_trade_search}`
> 关联链路：`docs/调用链/技能到房间结算.md`; `docs/调用链/跨设施状态刷新.md`
> 核验基线：`947f1a2`

## 搜索链

```text
OperBox + runtime data -> facility pool -> Plan/exclusion/used/capacity
 -> domain search -> comparator/policy -> hit/report
 -> assignment commit -> resolve -> next stage
```

pool/admission、容量、required anchor、互斥和输入匹配是 hard constraint。已证明等价的枚举才可称 safe reduction；top-k、排序、control policy、producer prefix 和 shadow branch 都是 policy/heuristic。完整池模式不能被 standalone 或 Bake 子集替代。

全布局黑键与巫恋不得同贸易站是用户裁决的产品 `hard_constraint`，由贸易单单种类搜索在排名和 top-K 前过滤，涵盖普通填房、role 和分站搜索；不依赖 Rosemary 是否激活。固定 seed / Plan 的非法同站先拒绝，最终班表再由 Rust invariant gate 检查。该限制不改游戏技能结算，也不禁止合法巫恋+龙舌兰组。

鸿雪杜林组成套核心的四条真实赤金制造线要求是 Plan `hard_constraint`；两线金单域搜索在散件缩池补入鸿雪、图耶，普通金贸两线填房还会独立枚举两人的同房候选。填房时对该组做带双人 required 的全贸易池搜索并按相同效率比较，不将这一条可选候选提升为 core、必选或全局最优保证。

## 域入口

`search_trade_triples`、`search_manufacture_triples`、`search_control_combos`、`search_power_assignment` 各自负责域内比较；`joint_trade` 只处理具名跨域 candidate policy。遇到“候选没被考虑”，按 pool skip → Plan claim/exclusion → used → capacity → domain filter → comparator 排查。

`ControlFillPolicy::LayeredFill` 当前走中枢专用顺序入口：先满足适用候选组，再按贸易、制造、训练（没有专精上下文时保持 `Unsupported`）、办公室、会客室、心情和普通补位逐位消费“当前已选 + 当前候选”。前序席位锁定后，后序候选不能用匿名加权分数替换它。`build_control_pool_with_fillers` 只增加真实没有设施技能的干员；带未建模 buff 的干员留在 `skipped`，不会被清空技能后重新加入。

该顺序入口的贸易/制造区只读取冻结 `ControlEffectLedger` 的静态全局 key；按人数缩放、当前房间、deferred producer 和动态 projection 留给后续 resolve，不得提前成为中枢区 winner。调用方提供的 ETA eligibility 在候选组、每个效果区、统一心情区和普通补位每次形成满容量候选时都会检查。候选组逐组贪心仍是具名 policy，不能把 `NO_CANDIDATE_UNDER_POLICY` 或预算耗尽解释成 canonical `INFEASIBLE`。

## Bake

`bake_catalogs` 用 live solver 生成 schema v13 catalog 和 input fingerprint。`validate_baked_catalog` 检查 schema、输入 hash、row 集与 combo table hash。`BakeMode::Auto` 不兼容就回 live，`Disabled` 禁用，`Required` 对 miss 失败。生成物不可手工编辑为规则。

黑键—巫恋产品禁配只在读取已验证完整的贸易 Bake 表时过滤，先排除非法行再取前 K 条；它本身不造成 `Required` miss。其他未表示的同站禁配仍由 `Auto` 回 live、`Required` 报错。catalog 的可执行文件指纹失效时照常 miss，需另行重建，不能拿旧表证明命中。

核验：`bake validate`、`bake verify`、`infra-cli verify`，并记录输入、组合数、evaluated 和命中模式。候选/剪枝/Bake 改动还要运行最终 assignment invariant gate。
