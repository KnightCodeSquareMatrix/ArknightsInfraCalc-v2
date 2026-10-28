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

`BAKE_SCHEMA_VERSION` 当前为 16。`bake_catalogs` 以同一 live solver 生成 catalog、manifest、operator/signature 和 combo table；manifest 保存当前可执行文件 hash、输入文件 fingerprint、生成选项、schema 和 row hash。exe 指纹成功与失败均在进程内缓存。`validate_baked_catalog` 在 schema、fingerprint、row 集或 hash 不匹配时拒绝；核心 `verify_baked_catalog_responses` 对每一行重算完整响应，另做确定性查询差分。

运行时 `BakeMode::Auto` 在兼容查询首次加载缺表/失效时生成完整表，全量核心校验通过才发布；生成和校验的 live 搜索显式 `Disabled`，RAII 标记防止递归。无参预热也用 Auto。进程内初始化及失败由 OnceLock 缓存，不在每次查询重复读取 exe；失败只 stderr 报告一次简短原因后回 live。`Disabled` 永不读取；`Required` 不主动生成，对任何 miss/error 失败。当前单元尚未把 miss 按类别接入 `[perf]`，计数留下一单元。

发布使用独立 generation 目录与 CURRENT 指针的原子 rename，生成目录验证后只读；同进程只初始化一次，跨进程通过不等待的短发布锁复核目标，输方读取有效胜方或回 live，不发布未验证/limited 表。校验、verify、完整贸易 API 和 debug 指纹统一解析 CURRENT；旧平铺表仍可校验。默认目录优先规则与首次 release 输出位置见 [候选搜索与 Bake](模块/候选搜索与Bake.md)。`data/baked/` 已忽略；取消历史跟踪和清理旧目录待主代理执行。生成物不能手工改成语义来源。

## 诊断与验证

性能报告至少记录 commit、输入 OperBox、pool size、组合数、evaluated、命中模式和 stage timing。正确性用 `bake validate`、`bake verify`、`infra-cli verify` 和最终 assignment gate；性能数据不能证明机制覆盖，全量行校验仍不能证明未覆盖查询上下文或全部子集排序的全局等价。

## 常驻性能汇总

启用 trace 的求解在 stderr 输出一条 `[perf]` JSON，诊断 schema 为 2、`event` 为 `solver_summary`；求解结果 JSON 不增加字段。trace guard 结束时集中格式化和写出，错误和提前返回也会释放；嵌套 trace 结束后恢复外层 trace，`serve` 每次请求单独汇总。`profile layout-full` 每次 run 建立自己的 trace，标签使用实际 layout 和 OperBox 路径。

默认只在固定的 61 个 scope/stage 槽里累计 `calls`、`errors`、`elapsed_ms` 和各 operation 的计数与耗时差量；热阶段不构造 JSON、不格式化、不写 stderr。输出仅列出至少一个非零字段的 operation，省略全零 operation。新增的未知标签安全累计到 `stage_id=61` 的 overflow 槽，保留调用、错误、耗时与 metrics，不影响求解。`errors` 统计调用方原有 `status="error"`，其中可能包含无候选命中，不能等同于请求失败。

全局 `--perf-detail` 才保留详细原始数值事件（包含原始静态标签与 status），在结束时写入同一汇总的 `details`；默认没有详细事件 Vec。`--no-perf` 优先关闭 trace 汇总和详细记录；已有边界计时、snapshot 和独立 hot-path 原子计数不因此全部消失，不能声称零开销。

`elapsed_ms` 从 trace 建立计到性能日志格式化前，`elapsed_boundary=trace_start_to_before_perf_format` 明确这一边界；不声称包含性能日志格式化或 stderr 输出耗时，`serve` 的后续响应序列化和写出也在 trace 之外。stage 和 operation 耗时是 inclusive，父子阶段及其 metrics 差量可能覆盖同一次操作，不可相加；汇总顶层 `metrics` 是当前 trace 的原始累计，不能用相加各 stage 代替。


## 实际调用计数与排班归因

贸易与制造的每次叶搜索在调用线程建立数字 batch，Rayon 的 `map_init` 为每个任务安装同一搜索共享计数的独立 TLS batch。solver/check 的真实入口只递增 TLS 普通整数；任务释放时按每种非零计数一次合并到该搜索共享原子和全局原子。串行准备、Bake 检查与制造前 K 条完整物化也在搜索 batch 内。嵌套 batch 释放后恢复外层 TLS，内层计数独立归属，避免重复累计；不在搜索 batch 内的直接入口继续递增全局原子。

`actual_trade_solves`、`actual_exclusive_checks`、`actual_shortcut_json_loads` 是每次搜索真实入口次数，在全部任务 batch 和串行 batch 释放后加入对应 operation。solver 早退、检查失败，以及算完后被 hit filter 排除的组合都保留已发生的次数；组合数、尝试数与最终命中数不能代替这些计数。制造轻量枚举和完整物化只统计已存在的这三类入口，本版本没有制造 solve 次数指标。顶层 operation 仅覆盖搜索归属的实际次数；全局 hot-path snapshot 还包含其他直接求解入口。

排班新增固定阶段覆盖 N/A/B 候选、全中枢重建的 `attempt_0` 至 `attempt_3`、γ 两半、深海候选构造及评分、菲亚单核和巫恋替补、源石对 peak 与普通 disjoint backup。候选和重建阶段包含最终化；一次候选中的其他内部重试包含在同一重建阶段。`calls` 是实际进入该阶段的次数，替补函数合法返回未应用也算成功调用。peak 和 ABC total 改为 RAII 计时，进入后错误提前返回也保留差量；其余新阶段同样以 RAII 保留错误退出。阶段是 inclusive，同一操作会出现在候选、重建和内部阶段中，禁止加总父子计数。

## 贸易房间解释器的借用边界

单次房间解释器借用输入干员的名字、技能 ID、标签及预编译 atom 切片；房间 runtime 与归并源用五槽 `SmallVec`，归并游标用固定五槽数组，人数超过五人时允许堆上溢出。归并按原 `phase/order → 输入 owner 序号 → atom seq` 次序逐条应用，不再构造、排序整房 atom 列表或逐 atom 克隆。手动构造且未携带 atom 的技能组合通过同一技能表的冷缓存补齐，包括合法空结果；仅补缺项临时持有 Arc。赤金链仍按输入顺序在 `PeerAbsorb` 前结算，订单上限重算时机不变。

当前房标签注入改为按请求标签遍历计数，重复标签每次出现仍计一次；注入 family 的 HashMap 克隆和浮点求和遍历保留。拥有名字的心情输出在输出边界分配，layout 仍共享 Arc，房间 state_pool 暂保持原表示。该单元属于内部执行成本优化，不改变候选集合、剪枝、目标或保证等级；本次仅格式化与 release 编译，未运行求解、Bake 或新旧输出对拍，不能据此声称性能提升幅度或全局机制等价。
