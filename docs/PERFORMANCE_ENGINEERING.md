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

## 常驻性能汇总

启用 trace 的求解在 stderr 输出一条 `[perf]` JSON，诊断 schema 为 2、`event` 为 `solver_summary`；求解结果 JSON 不增加字段。trace guard 结束时集中格式化和写出，错误和提前返回也会释放；嵌套 trace 结束后恢复外层 trace，`serve` 每次请求单独汇总。`profile layout-full` 每次 run 建立自己的 trace，标签使用实际 layout 和 OperBox 路径。

默认只在固定的 48 个 scope/stage 槽里累计 `calls`、`errors`、`elapsed_ms` 和各 operation 的计数与耗时差量；热阶段不构造 JSON、不格式化、不写 stderr。输出仅列出至少一个非零字段的 operation，省略全零 operation。新增的未知标签安全累计到 `stage_id=48` 的 overflow 槽，保留调用、错误、耗时与 metrics，不影响求解。`errors` 统计调用方原有 `status="error"`，其中可能包含无候选命中，不能等同于请求失败。

全局 `--perf-detail` 才保留详细原始数值事件（包含原始静态标签与 status），在结束时写入同一汇总的 `details`；默认没有详细事件 Vec。`--no-perf` 优先关闭 trace 汇总和详细记录；已有边界计时、snapshot 和独立 hot-path 原子计数不因此全部消失，不能声称零开销。

`elapsed_ms` 从 trace 建立计到性能日志格式化前，`elapsed_boundary=trace_start_to_before_perf_format` 明确这一边界；不声称包含性能日志格式化或 stderr 输出耗时，`serve` 的后续响应序列化和写出也在 trace 之外。stage 和 operation 耗时是 inclusive，父子阶段及其 metrics 差量可能覆盖同一次操作，不可相加；汇总顶层 `metrics` 是当前 trace 的原始累计，不能用相加各 stage 代替。
