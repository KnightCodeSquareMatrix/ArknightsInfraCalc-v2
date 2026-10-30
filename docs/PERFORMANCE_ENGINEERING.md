# 搜索性能与 Bake 边界

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/模块/候选搜索与Bake.md
> 代码范围：`crates/infra-core/src/{bake.rs,pool,search,candidate.rs,profile.rs}`; `crates/infra-cli/src/commands/{bake,profile}.rs`; `data/baked/`
> 代码锚点：`crates/infra-core/src/bake.rs::{bake_catalogs,validate_baked_catalog,try_baked_trade_search}`; `crates/infra-core/src/pool/base.rs::{build_roster_pool,filter_pool}`; `crates/infra-core/src/search/trade.rs::search_trade_triples`; `crates/infra-core/src/search/manufacture.rs::search_manufacture_triples`; `crates/infra-core/src/layout/assign/pipeline.rs::{run_shift_pipeline,run_shift_pipeline_with_producer_pruning}`
> 关联链路：`docs/模块/候选搜索与Bake.md`; `docs/模块/中枢与跨设施状态.md`; `docs/SCORING_MODEL.md`
> 核验基线：`947f1a2`

## 成本模型

主要成本是对候选组合运行真实 domain solver，并在部分 assignment、producer prefix 和 shadow branch 上重复 resolve。组合数由 pool size 和人数决定；报告的 `combinations` 与 `evaluated` 必须保留，不能只报 wall time。

## 可接受的优化

- `Pool` 过滤、房间容量、required admission、`used`、互斥和配方/订单是 hard constraint。
- 已证明等价的域内枚举可写为 safe reduction；top-k、稳定排序、control policy、producer prefix pruning 和 shadow branch 属于 policy/heuristic。
- 任何剪枝若不能证明等价，只能声称在该 policy 下搜索，不得声称全局最优。
- `full_pool` 不能被 standalone pool 或 Bake 子集静默替换；动态 context 必须重新检查。

## Bake 生命周期

`BAKE_SCHEMA_VERSION` 当前为 15。`bake_catalogs` 以同一 live solver 生成 catalog、manifest、operator/signature 和 combo table；manifest 保存输入文件 fingerprint、生成选项、schema 和 row hash。`validate_baked_catalog` 在 schema、fingerprint、row 集或 hash 不匹配时拒绝。

运行时 `BakeMode::Auto` 命中兼容行，否则回退 live；`Disabled` 永不读取；`Required` 对任何 miss/error 失败。Bake 生成期间禁止自读生成物。`bake verify` 用 live solver 对 signature 的首/中/尾响应抽样对账；生成物不能手工改成语义来源。

## 诊断与验证

性能报告至少记录 commit、输入 OperBox、pool size、组合数、evaluated、命中模式和 stage timing。正确性用 `bake validate`、`bake verify`、`infra-cli verify` 和最终 assignment gate；性能数据不能证明机制覆盖，Bake 校验也不能证明未抽样行的全局等价。

## 制造体系试排的条件响应缓存（2026-10-30）

Plan 的 `complete_manufacture_role` 和 `score_completed_manufacture` 使用制造域的运行时条件响应表。
旧磁盘制造 catalog 只覆盖统一 E2 静态散件，不能用于实际 Box 的完整制造池；本次不扩张该表的
兼容条件。运行时表通过原 `search_manufacture_triples` 全量生成并排序，保存每个合法组合，
随后用人员位集、必选成员和原比较器顺序查询。没有提前截断候选或按体系名缓存 winner。

表由本次加载的不可变 `SkillTable` 持有，克隆共享，只复用已生成的单房响应；不存 assignment、
used、Plan、Team 或轮换状态。每次查询核对技能表、实际 operator 的姓名/elite/buff/tag、候选
顺序、房间等级/容量/配方/心情、必选限制和结算上下文。候选必须为已生成池的有序子序列；
否则生成新表。非完整池、多产线和定时结算仍走原搜索。本缓存至多保留 16 张表，淘汰只增加
重复计算，不改变答案。它是 exact cache / safe reduction，不提高现有有序填房的最优性保证。

静态制造上下文保留全部数字、资源、制造标签总数和完整注入；仅消除不被静态制造读取的中枢
名单、贸易标签/产线来源，以及技能未点名且不参与候选去重的 workforce。候选自身对动态计数、
莱茵计数的投影仍由原 `manufacture_combo_layout` 完成。输入变化造成 miss 时仍运行同一个 live
solver。磁盘表的 schema/generator/input 验证和缺表回退保持原合同。

`[perf]` 中 `generated_combinations` 保留本次逻辑组合数，`candidate_attempts` 是实际重算数；
响应表命中记录 `bake_hits`，不会将条件表首次生成冒充磁盘 Bake 命中。

验收：同一 425 人 Box（`high-owned/b-231-425.json`）、release、Rayon 2 线程、默认三班 ABC、
菲亚开启、完整导出及终态 gate，交替两轮：原版 21.17/20.94s，修复版 7.77/7.97s；三个
候选的 Plan 构建合计约 15.2s → 1.8–1.9s。逻辑制造组合数均为 6,343,823，实际重算由
6,343,823 → 1,904,989，82/120 次制造搜索复用响应。样例峰值内存约 60–70MB → 210MB；
首次生成、定时普通填房和轮换仍有成本，不能据此宣称全程 1s 或所有 Box 同速。
421 次缓存/实时响应或错误对照通过（含缺必选、容量不足、定时回退、心情和技能表变化）；38 个持久 Box 的 profile/rotation/MAA 与基点一致，
终态 gate 全过。制造定向测试 108 过、1 条既有薇薇安娜断言失败；`bake validate` 通过，
`bake verify` 的响应抽样通过，但附带 `reg_gsl_docus_solo` 机制回归失败（基点也为相同
1.907/1.909 差异）。本轮未跑 full suite，不宣称全部机制回归通过。
证据：`target/codex-runs/manufacture-bake-20261030/`；性能测量不包含编译或生成磁盘表时间。
