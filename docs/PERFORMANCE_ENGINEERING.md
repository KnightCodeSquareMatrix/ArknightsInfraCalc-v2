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

自动轮换的逐候选协同准入还会复制站点成员和布局、重建实际资源来源；这些工作没有计入普通`resolve.calls`，不能用该计数判断全部跨设施状态成本。挂件修正后的三模式实测、函数采样质量与终态组合遍历规模见[排班性能评估](ARCHIVE/done/排班性能评估-20261029.md)；这是7322ecf的测量快照，后续优化尚未实施。

贸易与制造共用内部 `search/enumerate.rs` 的固定三槽字典序枚举：容量为 1..=3，按全部必含下标过滤，只为平铺的 `Vec<usize>` 分配组合载体，每 `k` 个下标代表一个组合。制造传入单锚下标，贸易传入原必含集合；贸易原 `combo_count` 公式保留，实际尝试数按平铺长度除以容量计算。公共 `combinations_indices` 及其其他调用者保留。

两域的求值闭包交给静态泛型并行 helper，沿用 `flat.par_chunks(k).enumerate().map_init(batch) → flatten → filter → collect` 的相对次序；闭包直接执行原域内求值，不使用 Box、动态分派或额外锁。贸易仍按原顺序先执行命中过滤，再执行禁配过滤，制造保留原 Light 与前 K 完整物化。排序、同分顺序、top-K、错误及枚举集合保持，本单元属于执行成本优化；仅格式化、workspace 离线编译与源代码顺序检查，未新增测试或运行 solver、Bake、性能测量。

## 可接受的优化

- `Pool` 过滤、房间容量、required admission、`used`、互斥和配方/订单是 hard constraint。
- 已证明等价的域内枚举可写为 safe reduction；top-k、稳定排序、control policy、producer prefix pruning 和 shadow branch 属于 policy/heuristic。
- 任何剪枝若不能证明等价，只能声称在该 policy 下搜索，不得声称全局最优。
- `full_pool` 不能被 standalone pool 或 Bake 子集静默替换；动态 context 必须重新检查。

## Bake 生命周期

全表生成与冷校验已使用完整姓名/精英阶段/有序 buff/标签身份、五类练度代表元的真实建池准入及同名组合互斥；热查询使用 checked colex rank 和有界 top-K 堆，并核对 canonical 相对顺序、完整布局、技能原文及编译 atom，生产候选政策保持现状。旧语义消融已完整执行152盒及full-e2两入口：成员、产出、rotation和MAA零差异，296叶仅同文件输入路径标签，原same5/changed147保留；覆盖内纯性能归因可关闭。新冻结连续暖机局部数值验收及指定P1/P2静态闭合已完成：1122full含72预期非有限错误，1050search中798获准精确一致、252由既有>=12h负心情扣减政策排除；数学千分及公开f64合同零差异，不能称所有搜索获准。最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。 普通段分段积分与f64微段求值边界保留，不能泛称所有实数精确。旧表全量对账不替代新语义验收，不推广任意查询或独立Required+peer-reset；证据与门禁见活动文档§8.1及当前真源。

`BAKE_SCHEMA_VERSION` 当前为 17。`bake_catalogs` 以同一 live solver 生成 catalog、manifest、operator/signature 和 combo table；manifest 保存当前可执行文件 hash、输入文件 fingerprint、生成选项、schema 和 row hash。exe 指纹成功与失败均在进程内缓存。`validate_baked_catalog` 在 schema、fingerprint、row 集或 hash 不匹配时拒绝；核心 `verify_baked_catalog_responses` 对每一行重算完整响应，另做确定性查询差分。

运行时 `BakeMode::Auto` 在兼容查询首次加载缺表/失效时生成完整表，全量核心校验通过才发布；生成和校验的 live 搜索显式 `Disabled`，RAII 标记防止递归。无参预热也用 Auto。进程内初始化及失败由 OnceLock 缓存，不在每次查询重复读取 exe；失败只 stderr 报告一次简短原因后回 live。`Disabled` 永不读取；`Required` 不主动生成，对任何 miss/error 失败。miss 已按固定类别接入 `[perf]`，热查询只累计整数，结束时统一格式化。

发布使用独立 generation 目录与 CURRENT 指针的原子 rename，生成目录验证后只读；同进程只初始化一次，跨进程通过不等待的短发布锁复核目标，输方读取有效胜方或回 live，不发布未验证/limited 表。校验、verify、完整贸易 API 和 debug 指纹统一解析 CURRENT；旧平铺表仍可校验。默认目录优先规则与首次 release 输出位置见 [候选搜索与 Bake](模块/候选搜索与Bake.md)。`data/baked/` 已忽略；历史跟踪和旧目录已清理。生成物不能手工改成语义来源。

## 诊断与验证

性能报告至少记录 commit、输入 OperBox、pool size、组合数、evaluated、命中模式和 stage timing。正确性用 `bake validate`、`bake verify`、`infra-cli verify` 和最终 assignment gate；性能数据不能证明机制覆盖，全量行校验仍不能证明未覆盖查询上下文或全部子集排序的全局等价。

## 常驻性能汇总

启用 trace 的求解在 stderr 输出一条 `[perf]` JSON，诊断 schema 为 2、`event` 为 `solver_summary`；求解结果 JSON 不增加字段。trace guard 结束时集中格式化和写出，错误和提前返回也会释放；嵌套 trace 结束后恢复外层 trace，`serve` 每次请求单独汇总。`profile layout-full` 每次 run 建立自己的 trace，标签使用实际 layout 和 OperBox 路径。独立自动轮换 plan 也在加载前建立同一 trace，消除提前分派缺失性能汇总的问题。

默认只在固定的 61 个 scope/stage 槽里累计 `calls`、`errors`、`elapsed_ms` 和各 operation 的计数与耗时差量；热阶段不构造 JSON、不格式化、不写 stderr。输出仅列出至少一个非零字段的 operation，省略全零 operation。新增的未知标签安全累计到 `stage_id=61` 的 overflow 槽，保留调用、错误、耗时与 metrics，不影响求解。`errors` 统计调用方原有 `status="error"`，其中可能包含无候选命中，不能等同于请求失败。

全局 `--perf-detail` 才保留详细原始数值事件（包含原始静态标签与 status），在结束时写入同一汇总的 `details`；默认没有详细事件 Vec。`--no-perf` 优先关闭 trace 汇总和详细记录；已有边界计时、snapshot 和独立 hot-path 原子计数不因此全部消失，不能声称零开销。

`elapsed_ms` 从 trace 建立计到性能日志格式化前，`elapsed_boundary=trace_start_to_before_perf_format` 明确这一边界；不声称包含性能日志格式化或 stderr 输出耗时，`serve` 的后续响应序列化和写出也在 trace 之外。stage 和 operation 耗时是 inclusive，父子阶段及其 metrics 差量可能覆盖同一次操作，不可相加；汇总顶层 `metrics` 是当前 trace 的原始累计，不能用相加各 stage 代替。


## 实际调用计数与排班归因

贸易与制造的每次叶搜索在调用线程建立数字 batch，Rayon 的 `map_init` 为每个任务安装同一搜索共享计数的独立 TLS batch。solver/check 的真实入口只递增 TLS 普通整数；任务释放时按每种非零计数一次合并到该搜索共享原子和全局原子。串行准备、Bake 检查与制造前 K 条完整物化也在搜索 batch 内。嵌套 batch 释放后恢复外层 TLS，内层计数独立归属，避免重复累计；不在搜索 batch 内的直接入口继续递增全局原子。

`actual_trade_solves`、`actual_exclusive_checks`、`actual_shortcut_json_loads` 是每次搜索真实入口次数，在全部任务 batch 和串行 batch 释放后加入对应 operation。solver 早退、检查失败，以及算完后被 hit filter 排除的组合都保留已发生的次数；组合数、尝试数与最终命中数不能代替这些计数。制造轻量枚举和完整物化只统计已存在的这三类入口，本版本没有制造 solve 次数指标。顶层 operation 仅覆盖搜索归属的实际次数；全局 hot-path snapshot 还包含其他直接求解入口。

排班固定阶段覆盖 N/A/B 候选、γ 两半、深海候选构造及评分、菲亚单核和巫恋替补、源石对 peak 与普通 disjoint backup。候选阶段包含构造及终态结算；已删除的中枢重建与主备撤预算重试不再执行。`calls` 是实际进入该阶段的次数，替补函数合法返回未应用也算成功调用。peak 和 ABC total 改为 RAII 计时，进入后错误提前返回也保留差量；其余新阶段同样以 RAII 保留错误退出。阶段是 inclusive，同一操作会出现在候选和内部阶段中，禁止加总父子计数。

## 贸易房间解释器的借用边界

`TradeOperatorRef` 借用姓名、技能 ID、标签及 atom 切片，`TradeRoomView` 自持五槽引用 `SmallVec` 并完整携带原房间 scalar，借用共享 layout。公开 owned 输入与 `TradeContext::from_room` 保留，context 通过临时 view 进入同一初始化链，借用生命周期只绑定底层身份；人间烟火覆盖、灵知初始注入和浮点顺序保留。热搜索的单组合入口经借用池 helper、房间 view、solver 单一内部实现贯通到 shortcut/segment 的 refs 内核，不再在每个组合复制姓名、技能 ID、标签或克隆预编译 Arc。override 选择及赤金链稳定排序保持；公开 owned solver API 和 Bake 使用的 owned prevalidated 入口通过 `as_view` 进入同一求解链。

单次房间解释器借用输入干员的名字、技能 ID、标签及预编译 atom 切片；房间 runtime 与归并源用五槽 `SmallVec`，归并游标用固定五槽数组，人数超过五人时允许堆上溢出。归并按原 `phase/order → 输入 owner 序号 → atom seq` 次序逐条应用，不再构造、排序整房 atom 列表或逐 atom 克隆。手动构造且未携带 atom 的技能组合通过同一技能表的冷缓存补齐，包括合法空结果；仅补缺项临时持有 Arc。赤金链仍按输入顺序在 `PeerAbsorb` 前结算，订单上限重算时机不变。

当前房标签注入改为按请求标签遍历计数，重复标签每次出现仍计一次；注入 family 的 HashMap 克隆和浮点求和遍历保留。拥有名字的心情输出在输出边界分配，layout 仍共享 Arc，房间 state_pool 的固定快照边界见下文。该单元属于内部执行成本优化，不改变候选集合、剪枝、目标或保证等级；本次仅格式化与 release 编译，未运行求解、Bake 或新旧输出对拍，不能据此声称性能提升幅度或全局机制等价。

借用类型与转换小单元仅格式化、workspace 离线编译及源差异检查；未新增测试或运行 solver、Bake、性能测量，未验证运行结果或性能幅度。

贸易 shortcut 的互斥、站型识别与匹配内核，以及注册 segment 的 consumer 转发和匹配内核，已统一读取 `TradeOperatorRef` 切片。原公开 owned 入口保留，通过一次五槽 `SmallVec` 引用转换进入同名 `_refs` 入口；内核互相调用直接传引用，私有身份 helper 也使用借用输入，一种判定只保留一份实现。注册链段优先级、shortcut 回退次序、互斥计数位置、tier 判定和命中 entry 的克隆保持。

候选投影和命中构造直接读取 refs；动态 layout 的工作名单、标签计数与命中姓名仍在输出边界拥有。热搜索仍完整构造 `TradeResult` 和命中，枚举、过滤、排序与轻量评分边界未改。没有调用者的旧内部 projection helper 和但书 owned tier 过渡 wrapper 已删除。贯通单元仅 owner 文件格式化、workspace 离线编译及源差异检查；未新增测试或运行 solver、Bake、性能测量，未验证性能幅度或全局机制等价。


## 制造房间解释器的借用边界

制造候选在建池或有技能表的构造阶段预备 `Option<Arc<[CompiledAtom]>>`；`Some` 空切片表示合法的已编译空技能，`None` 仅用于尚无技能表或只检查 buff 的输入。制造缓存独立按有序 buff ID 组合索引，跳过未知 ID 和非制造设施技能。单组合入口、热轻量评分及前 K 条完整物化通过 `ManuOperatorRef` 借用池内姓名、技能 ID、标签和 atoms，`ManuRoomView` 自持五槽引用 `SmallVec`；不再逐组合复制这些身份或克隆预编译 Arc。公开 owned 房间与 solver API 保留，通过临时 view 进入同一内部求解链；解释器生命周期绑定真实身份，不依赖 view 本身的借用时长。

单次房间 runtime 借用名字、技能 ID、标签与 atom 切片，干员和归并源使用五槽 `SmallVec`，游标使用五槽数组，超出时保留溢出能力。缺少预备结果的输入由同一技能表的冷缓存补齐并临时持有 Arc；按原 `phase/order → 输入 owner 序号 → atom seq` 顺序边归并边应用，不再收集和排序整房 atoms，不克隆 owner 名字。标签控制注入借共享 layout 的规则切片，拥有名字的心情输出仍在输出边界分配。

轻量评分在 `None` 或不存在 `OwnerMoodGap` selector/condition 的有限时长输入上执行完整 phases，仅读取原 solver 的效率转换与仓库表达式，跳过 `ManuResult` 及心情输出的两次 Vec 物化。非有限时长仍失败；存在心情差效果的定时输入继续进入原定时 solver（包括其非正时长回退），保留阈值分段、逐项时长平均和结束时刻仓库。定时 `DrainInputs` 的拥有姓名名单、动态 layout 的工作名单和标签计数、最终搜索命中的姓名均在原边界物化。

此单元是内部执行成本优化；配方浮点累加、peer reset、条件及仓库保持原实现，房间 state_pool 的固定快照边界见下文；轻量评分及前 K 条完整物化边界保留。不改变枚举、候选集合、剪枝、目标、同分顺序、K=0、evaluated 或保证等级。本次仅格式化和 workspace 离线编译，未运行 solver、Bake 或新旧输出对拍，不能据此声称性能提升幅度或全局机制等价。后续按用户裁决改为连续暖机积分：有班长取真实时长，无班长默认 20 小时；轻量与完整共用积分入口，技能面分桶在各时段结算后加权，详见制造状态文档。

## 制造全局注入的实例缓存

`GlobalInjectManifest::manu_eff_for` 用四个 `OnceLock<f64>` 槽按配方保存本实例首次结算值。冷路径仍由单一 `manu_eff_for_uncached` 执行原 family map 克隆、动态规则原序插入与同族取 max、两个 map 分别按实际 `values()` 次序求和后相加的算法；没有改成排序求和。热读取直接取对应槽位，缓存不进入 serde 输出、布局精确键或业务 `PartialEq`。

`record_manu`、`record_manu_count_scaled` 和 `refresh_manu_count_scaled` 每次调用均清空四槽；即使静态 max 值未变，也不能跳过失效，因为插入或容量变化可能影响后续求和顺序。同源 `Clone` 可复制已算值，后续制造写入口仍重新失效；除此之外不同动态实例之间没有缓存复用，布局 memo 原实例隔离保持。标签制造注入和 active buff 不参与此项数值读取，沿用各自结算路径。

这是内部重复执行成本优化，不改变 HashMap 领域接口、浮点表达式、规则顺序、候选或输出合同。本次仅格式化、workspace 离线编译与源差异检查；未新增测试或运行 solver、Bake、性能测量，不能据此证明性能幅度或全局机制等价。


## 贸易与制造房间的固定状态快照

两解释器的 `state_pool` 使用 `RoomState` 固定 16 个 `f64` 槽位，与 `GlobalResourceKey::ALL` 的现有资源集合对应；缺省槽位为 `0.0`。房间内原来全部读取都以缺键为零，不依赖键存在性、遍历顺序或回写全局池；现在通过状态键索引读取，`StateProduce` 与 `StateConvert` 直接 `+=`，贸易人间烟火显式输入仍覆盖初值。转化先读取源值再累加目标，即使源和目标相同也保持原顺序；乘法、下取整及后续浮点累加表达式不变。

`GlobalResourcePool` 继续使用 HashMap，并保留 `contains`、`iter`、`to_room_state` 和回写接口；中枢、办公室与发电站沿用原快照。新增 `to_fixed_room_state` 仅把现有全局项按键复制到房间槽位，不做浮点运算或归并。该优化不改变候选、phase 应用顺序、输出合同或保证等级。格式化、release 离线编译及差异检查通过仅证明构建与源代码边界；本次未运行求解、Bake、对拍或新增测试，未验证性能幅度和全局机制等价。


内部 `ExactKeySerializer` 已接入请求 memo 的布局、贸易与制造完整键：完整类型/字段/容器边界、原始浮点 bit 和 map 实际迭代顺序进入键，布局与注入的全部业务字段及预编译 atom 可直接编码。最终统一包及旧语义消融已完成覆盖内输出对照；真实命中率和额外性能收益未单独测量，不把原实现阶段编译证据泛化为普遍等价。

## Bake signature 的固定槽位与目录隔离

生产 rank 查询只接受现有 13 种合法 signature，以四个贸易槽和九个制造槽取代字符串 HashMap。level 必须在 1–3 且 capacity 等于等级；贸易赤金的有效金线数必须等于冷加载的 search baseline，源石继续沿原 signature 不限制金线。冷加载从各 index 首行重建原字符串 key 并核对，拒绝不支持或重复槽；磁盘 v17、完整 signature 集合与逐行验证、rank 填充、heap、门禁和比较顺序不变。这是内部查找成本优化，不扩大 Bake 支持域。正常 signature miss 不构造诊断字符串，Required 才延迟格式化原 key。

`ARKNIGHTS_INFRA_BAKED_DIR` 单独指定默认 Bake 目录，设置后直接使用，不参与其他输入文件的 `data_path` 发现。它可将最终验收生成和运行时加载都隔离到临时目录，保持默认 layout/baseline 标签与冻结产物一致；未设置时完全保留原目录发现顺序，CLI 显式 `--out` 继续优先。

本单元仅格式化、workspace 离线编译和源差异检查；未运行 solver、Bake、测量或测试，不能据此证明运行等价或性能幅度。最终全量 Bake 验证、产物差分和终态断言集中执行。

## Bake 生成与常驻表的内存边界（2026-10-29）

生成按 signature 求值、完整排序并应用原 limit，然后独立落盘行块并释放该 signature 的行；固定组合迭代器通过 `par_bridge` 求值，不再先收集全部组合。最后按原 facility/signature 顺序拼接 schema17 的 header 与行块，空块不生成索引。长度前缀、字段顺序、fixint 小端编码与 `DefaultHasher` 的整段 bytes 合同保留；流式 hash 先写整段长度，再分块写字节。临时块由 RAII 只清理本次拥有的路径，不降低候选覆盖或数值精度。

运行时每个 signature 保存 typed compact 行：贸易静态行大小预计 96B、制造 48B，这是类型布局估计，不是 RSS 测量。行保留有序 `u16` 状态下标、全部原始 `i32`/`f64`，rule 字符串以 checked `u32` 字典编号复用，colex rank 数组也使用 checked `u32` 与 sentinel。姓名、mask、facility 和机制元数据不再逐行常驻；选中行按 header 身份物化原 owned 命中，热查询没有 I/O，枚举、合法过滤、比较和输出合同保持。

冷加载对同一已打开 File 分块 hash 后 rewind，流式读取 metadata 与单行，核对全部结构、排序、身份与实际合法组合。完整合法 universe 用 rank bitset 枚举清位，拒绝缺行、残行及数量不符；canonical 有序 IDs 仍重建并核对贸易链重排，不把状态集合当作可交换输入。全量 verify 按 signature offset 重读同一 File，每批 4096 行核对全部磁盘字段与 live，并额外比较实际 compact 物化命中；确定性 320 查询及 skip 口径保持。完整贸易 API 保留 signature+offset 标识与原签名，按需输出 owned 行；显式索取全 signature 的 owned Vec 仍可能有较大分配。

此轮改动属于保持完整表和精度的内部成本优化，声明为 `safe reduction`。格式化、release 编译及静态契约/意图审查已完成；第一轮历史完整Bake的29,783,417行/3,292查询零差异、2,148跳过，CLI exit1仅但书旧期望；当轮内存观测9.2G/swap0B保留为历史。最终新语义完整Bake exit0/但书PASS；最终运行的cgroup采样与systemd终止摘要是不同观测口径，不拼接为单一RSS结论。第一轮四轮换/全精二对拍的选人、MAA及失败集合差异保留，五例24/24终态超预算断言已证实无误报，正常warning fallback与显式gate各按既定职责执行。用户最新裁决采用连续线性暖机：阿罗玛首小时整个0以后+2%/h至20%，其余四技能首小时20/15%以后+1/2%/h至25%；连续积分首轮独立数值验证已通过；P1铅踝扣减后槐琥漏读与P2合法极短浮点段拒绝已修复并完成新release编译，槐琥读取在全部EffVar直接贡献之后（order30、五ramp9保持），新冻结局部数值验收与指定P1/P2静态闭合已完成，1122full含72预期非有限错误、798获准search精确一致、252既有>=12h政策排除；最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。 保留无暖机热路径。旧语义完整消融的152盒及full-e2两入口中生产字段零差异，296叶仅同文件路径标签，原same5/changed147保留；覆盖内纯性能归因已关闭，不推广任意输入或Required+peer-reset。先平均再槐琥floor/cap不交换已确定，不能将旧整小时平均视为精确积分。暖机源码修改会改变`None`的20h表数值和exe指纹；原binary与旧表/对账证据保留，仅证明内存实现按旧暖机语义等价。最终连续暖机binary隔离完整表verify及CLI回归已完成，CLI exit0；不手改指纹，也不能用旧完整表验收替代新语义验收。本轮生命周期算法未改，冷路径覆盖来自完整Bake全量verify/Required查询与后续Auto加载，其他fresh/cache/fallback边界按实际覆盖报告，不追加重生成或性能测量。性能实现与最终实测已交付收口，五既有预算失败保持；没有额外热性能收益测量结论。执行状态与唯一冻结基线见 [性能优化调查与 Bake 重设计](TODO/性能优化调查与Bake重设计.md) §0.1。

## Bake 查询分类、冷耗时与稀疏日志

两域公开 `try_baked_*` 每次最多累计一个固定 `BakeMissKind`；compatibility 返回 typed 原因并只判断一次，内部 report helper、verify、load 与 warm 不重复记查询 miss。生成中和失效/不可用分别记 `generating` / `unavailable`；Required 按原原因冷格式化错误，Auto 不格式化 miss 诊断。原 rank/row/物化 Err 保留，单独计 query_errors；成功空制造报告和 rank sentinel 不算 miss。

冷阶段 init/generate/validate/verify/index/publish 以线程本地数字 calls/errors/nanos 累积，guard 保留错误或展开路径。真正 OnceLock initializer 才记录 init；CLI bake 的动作 trace 和 serve 启动同步 core warm 的独立 trace 让冷成本可见。serve 不再保留返回 None/false 的空桩，不引入后台生成竞态；warm 失败继续 live 与 ready。核心 Bake 校验、CLI 回归和已有错误不改变。

冷指标与查询分类加入 snapshot 的累计/差分，trace 结束才映射标签和生成 JSON；没有 trace 不计数。operation 与 Bake 指标省略数值零项，schema 2 消费者须将缺字段解释为零。阶段框架字段保持；固定数值收集不向组合热循环引入 format/JSON/I/O。父子冷耗时 inclusive，禁止加总；真实重复验证照次累计。

本单元仅格式化、workspace 离线编译及文档/源差异检查；未运行求解、Bake、测量或测试，不能据此声称性能幅度或最终行为等价。
性能报告至少记录 commit、输入 OperBox、pool size、组合数、evaluated、命中模式和 stage timing。正确性用 `bake validate`、`bake verify`、`infra-cli verify` 和最终 assignment gate；性能数据不能证明机制覆盖，Bake 校验也不能证明未抽样行的全局等价。

## 制造体系试排的缓存取舍（2026-10-31）

`complete_manufacture_role` 与 `score_completed_manufacture` 直接调用原实时制造搜索，保留请求memo、实际预算与排序。已删除额外条件响应表及其计时阶段：完整动态上下文和保守预算使跨试排复用有限，首次全量生成和排序的成本超过收益。

最终候选缓存版本与实时搜索消融的12对CLI结果共28对rotation/profile/MAA产物一致；定向response除elapsed的完整best/top/combinations/evaluated也一致。串行release、Rayon2、慢盒各3次，三班b-102-421中位数9.69s→4.41s，b-231-425约9.53s→5.09s；自动b-101-424约18.18s→实时约12s。最终发布版本速度以本轮整合归档的全样本测量为准；旧静态缓存结果不能代表真实定时预算下的收益。

本次属于执行成本优化，候选、排序、top-K及保证等级不变。不恢复静态裁剪来绕过真实心情准入，也不提高全局最优保证。证据保留于`target/codex-runs/main-integration-20261031/`。

## 主分支整合的最终测速（2026-10-31）

同一最终release二进制、Rayon2/OMP1，独立CLI串行运行，包含数据加载、求解、显式终态断言及产物写入；生成Bake的进程暂停，文件系统已暖。28代表性盒和10高持有量盒各测三班、12/12/12、auto24/12，共114次；普通三班及自动模式各取最慢两盒重复三次，共12次。126次全部成功。

| 模式 | 代表性28盒：中位/最大 | 高持有量10盒：中位/最大 |
|---|---|---|
| 三班 `3` | 0.424s / 2.824s | 3.078s / 4.618s |
| 三班 `12/12/12` | 0.375s / 2.457s | 2.315s / 4.001s |
| 自动 `auto@24/12` | 0.768s / 2.037s | 2.163s / 11.991s |

三班b-102-421重复4.418–4.510s，b-231-425为4.836–5.154s；自动b-101-424为11.866–14.435s，b-112-404为6.744–7.547s。自动最慢盒制造搜索约10.8s、226万候选；三班最慢盒制造搜索约3.4s、318万候选。计时是包含子调用的口径，不能累加阶段。仍有自动模式慢盒；这些结果只代表当前机器、线程与输入集合，不证明任意Box时延或线上吞吐。

最终二进制SHA-256为`f4f672ba8f478a9b93296e28257ee80caf89ca46f87631eef417e2685a39c7a8`；完整命令、逐盒指标和重复测量见`target/codex-runs/main-integration-20261031/final-speed-summary.json`及`speed/final-live-*`。

## 最终整合Bake（2026-10-31）

同一最终release二进制在隔离目录执行`bake all`，生成schema17共29,783,417行；发布前及原子发布后均完成完整结构/逐行/live查询校验。命令exit0，最终报告29,783,417行零差异、3,292查询零差异、2,148按既有合同跳过、28项机制回归通过。完整冷流程约1112秒；generate/validate/verify/publish是inclusive观测，不可相加，运行中曾为独占速度测量暂停。二进制和表指纹一致；未部署。
