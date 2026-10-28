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

全表生成与冷校验已使用完整姓名/精英阶段/有序 buff/标签身份、五类练度代表元的真实建池准入及同名组合互斥；热查询使用 checked colex rank 和有界 top-K 堆，并核对 canonical 相对顺序、完整布局、技能原文及编译 atom，生产候选政策保持现状。最终行/查询差分和运行验收尚未执行，门禁与边界详见当前真源。

`BAKE_SCHEMA_VERSION` 当前为 17。`bake_catalogs` 以同一 live solver 生成 catalog、manifest、operator/signature 和 combo table；manifest 保存当前可执行文件 hash、输入文件 fingerprint、生成选项、schema 和 row hash。exe 指纹成功与失败均在进程内缓存。`validate_baked_catalog` 在 schema、fingerprint、row 集或 hash 不匹配时拒绝；核心 `verify_baked_catalog_responses` 对每一行重算完整响应，另做确定性查询差分。

运行时 `BakeMode::Auto` 在兼容查询首次加载缺表/失效时生成完整表，全量核心校验通过才发布；生成和校验的 live 搜索显式 `Disabled`，RAII 标记防止递归。无参预热也用 Auto。进程内初始化及失败由 OnceLock 缓存，不在每次查询重复读取 exe；失败只 stderr 报告一次简短原因后回 live。`Disabled` 永不读取；`Required` 不主动生成，对任何 miss/error 失败。当前单元尚未把 miss 按类别接入 `[perf]`，计数留下一单元。

发布使用独立 generation 目录与 CURRENT 指针的原子 rename，生成目录验证后只读；同进程只初始化一次，跨进程通过不等待的短发布锁复核目标，输方读取有效胜方或回 live，不发布未验证/limited 表。校验、verify、完整贸易 API 和 debug 指纹统一解析 CURRENT；旧平铺表仍可校验。默认目录优先规则与首次 release 输出位置见 [候选搜索与 Bake](模块/候选搜索与Bake.md)。`data/baked/` 已忽略；历史跟踪和旧目录已清理。生成物不能手工改成语义来源。

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

`TradeOperatorRef` 借用姓名、技能 ID、标签及 atom 切片，`TradeRoomView` 自持五槽引用 `SmallVec` 并完整携带原房间 scalar，借用共享 layout。公开 owned 输入与 `TradeContext::from_room` 保留，context 通过临时 view 进入同一初始化链，借用生命周期只绑定底层身份；人间烟火覆盖、灵知初始注入和浮点顺序保留。池已提供借用身份与组合 helper，override 选择及赤金链稳定排序照原 owned helper；借用候选投影判定也已准备。本小单元仅增加数据转换，solver、search、shortcut 与现 owned 组合 helper 调用者尚未改接借用入口，因此不据此声称逐组合复制已消除。

单次房间解释器借用输入干员的名字、技能 ID、标签及预编译 atom 切片；房间 runtime 与归并源用五槽 `SmallVec`，归并游标用固定五槽数组，人数超过五人时允许堆上溢出。归并按原 `phase/order → 输入 owner 序号 → atom seq` 次序逐条应用，不再构造、排序整房 atom 列表或逐 atom 克隆。手动构造且未携带 atom 的技能组合通过同一技能表的冷缓存补齐，包括合法空结果；仅补缺项临时持有 Arc。赤金链仍按输入顺序在 `PeerAbsorb` 前结算，订单上限重算时机不变。

当前房标签注入改为按请求标签遍历计数，重复标签每次出现仍计一次；注入 family 的 HashMap 克隆和浮点求和遍历保留。拥有名字的心情输出在输出边界分配，layout 仍共享 Arc，房间 state_pool 的固定快照边界见下文。该单元属于内部执行成本优化，不改变候选集合、剪枝、目标或保证等级；本次仅格式化与 release 编译，未运行求解、Bake 或新旧输出对拍，不能据此声称性能提升幅度或全局机制等价。

借用类型与转换小单元仅格式化、workspace 离线编译及源差异检查；未新增测试或运行 solver、Bake、性能测量，未验证运行结果或性能幅度。


## 制造房间解释器的借用边界

制造候选在建池或有技能表的构造阶段预备 `Option<Arc<[CompiledAtom]>>`；`Some` 空切片表示合法的已编译空技能，`None` 仅用于尚无技能表或只检查 buff 的输入。制造缓存独立按有序 buff ID 组合索引，跳过未知 ID 和非制造设施技能。单组合入口、热轻量评分及前 K 条完整物化通过 `ManuOperatorRef` 借用池内姓名、技能 ID、标签和 atoms，`ManuRoomView` 自持五槽引用 `SmallVec`；不再逐组合复制这些身份或克隆预编译 Arc。公开 owned 房间与 solver API 保留，通过临时 view 进入同一内部求解链；解释器生命周期绑定真实身份，不依赖 view 本身的借用时长。

单次房间 runtime 借用名字、技能 ID、标签与 atom 切片，干员和归并源使用五槽 `SmallVec`，游标使用五槽数组，超出时保留溢出能力。缺少预备结果的输入由同一技能表的冷缓存补齐并临时持有 Arc；按原 `phase/order → 输入 owner 序号 → atom seq` 顺序边归并边应用，不再收集和排序整房 atoms，不克隆 owner 名字。标签控制注入借共享 layout 的规则切片，拥有名字的心情输出仍在输出边界分配。

轻量评分在 `None` 或不存在 `OwnerMoodGap` selector/condition 的有限时长输入上执行完整 phases，仅读取原 solver 的效率转换与仓库表达式，跳过 `ManuResult` 及心情输出的两次 Vec 物化。非有限时长仍失败；存在心情差效果的定时输入继续进入原定时 solver（包括其非正时长回退），保留阈值分段、逐项时长平均和结束时刻仓库。定时 `DrainInputs` 的拥有姓名名单、动态 layout 的工作名单和标签计数、最终搜索命中的姓名均在原边界物化。

此单元是内部执行成本优化；配方浮点累加、peer reset、条件及仓库保持原实现，房间 state_pool 的固定快照边界见下文；暖机仍按原 20 小时结算，轻量评分及前 K 条完整物化边界保留。不改变枚举、候选集合、剪枝、目标、同分顺序、K=0、evaluated 或保证等级。本次仅格式化和 workspace 离线编译，未运行 solver、Bake 或新旧输出对拍，不能据此声称性能提升幅度或全局机制等价。

## 制造全局注入的实例缓存

`GlobalInjectManifest::manu_eff_for` 用四个 `OnceLock<f64>` 槽按配方保存本实例首次结算值。冷路径仍由单一 `manu_eff_for_uncached` 执行原 family map 克隆、动态规则原序插入与同族取 max、两个 map 分别按实际 `values()` 次序求和后相加的算法；没有改成排序求和。热读取直接取对应槽位，缓存不进入 serde 输出、布局精确键或业务 `PartialEq`。

`record_manu`、`record_manu_count_scaled` 和 `refresh_manu_count_scaled` 每次调用均清空四槽；即使静态 max 值未变，也不能跳过失效，因为插入或容量变化可能影响后续求和顺序。同源 `Clone` 可复制已算值，后续制造写入口仍重新失效；除此之外不同动态实例之间没有缓存复用，布局 memo 原实例隔离保持。标签制造注入和 active buff 不参与此项数值读取，沿用各自结算路径。

这是内部重复执行成本优化，不改变 HashMap 领域接口、浮点表达式、规则顺序、候选或输出合同。本次仅格式化、workspace 离线编译与源差异检查；未新增测试或运行 solver、Bake、性能测量，不能据此证明性能幅度或全局机制等价。


## 贸易与制造房间的固定状态快照

两解释器的 `state_pool` 使用 `RoomState` 固定 16 个 `f64` 槽位，与 `GlobalResourceKey::ALL` 的现有资源集合对应；缺省槽位为 `0.0`。房间内原来全部读取都以缺键为零，不依赖键存在性、遍历顺序或回写全局池；现在通过状态键索引读取，`StateProduce` 与 `StateConvert` 直接 `+=`，贸易人间烟火显式输入仍覆盖初值。转化先读取源值再累加目标，即使源和目标相同也保持原顺序；乘法、下取整及后续浮点累加表达式不变。

`GlobalResourcePool` 继续使用 HashMap，并保留 `contains`、`iter`、`to_room_state` 和回写接口；中枢、办公室与发电站沿用原快照。新增 `to_fixed_room_state` 仅把现有全局项按键复制到房间槽位，不做浮点运算或归并。该优化不改变候选、phase 应用顺序、输出合同或保证等级。格式化、release 离线编译及差异检查通过仅证明构建与源代码边界；本次未运行求解、Bake、对拍或新增测试，未验证性能幅度和全局机制等价。


U4a 提供内部 `ExactKeySerializer`：完整类型/字段/容器边界、原始浮点 bit 和 map 实际迭代顺序进入键，布局与注入的全部业务字段及预编译 atom 可直接编码；该模块尚未接入搜索或 memo，本阶段仅格式化、release 离线编译及差异检查，不能据此证明缓存等价或性能收益。
