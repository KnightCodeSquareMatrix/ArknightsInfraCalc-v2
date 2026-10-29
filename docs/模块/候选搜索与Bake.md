# 候选搜索与 Bake

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/SCORING_MODEL.md; docs/PERFORMANCE_ENGINEERING.md
> 机制依赖：docs/知识库引用.md；vault:docs/1-基础设定/设施与进驻/基建总览.md；vault:docs/3-组合库/组合注册表.md
> 摘要：定位候选池、Plan 门禁、域搜索、排序与 Bake 回退的完整链路。
> 代码范围：`crates/infra-core/src/{pool,search,bake.rs,candidate.rs}`; `crates/infra-cli/src/commands/{bake,profile}.rs`; `data/baked/`
> 代码锚点：`crates/infra-core/src/pool/base.rs::{build_roster_pool,filter_pool}`; `crates/infra-core/src/search/{trade,manufacture,control,power,joint_trade}.rs`; `crates/infra-core/src/bake.rs::{bake_catalogs,validate_baked_catalog,try_baked_trade_search}`
> 关联链路：`docs/调用链/技能到房间结算.md`; `docs/调用链/跨设施状态刷新.md`
> 核验基线：`01cbec5`; `target/codex-runs/trade-final-efficiency-unit-20261020-r2/`; `out/trade-final-efficiency-unit-20261020/baked-final-r2/`

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

## 请求内搜索 memo

贸易单订单和制造单配方叶入口在各自过滤、组合准备之前查询请求内 memo。这属于完整输入精确相等的 `safe reduction`：复用成功 report，不更改候选、必选、禁配、排序、top-K 或组合计数；`Stations` / `Lines` 包装器仍分别调用叶搜索并自行组合报告。叶入口的错误不缓存，`BakeMode::Required` 和带贸易 hit predicate 的查询不缓存；键编码失败保守执行原搜索。

键覆盖不可变 SkillTable identity、有序完整 pool entry（含练度、buff、标签、预编译 atom、hint、mechanic/delegate、tier）、完整 LayoutContext 及其私有 global 数据、全部 options 和叶订单/配方、贸易 must/完整 override/禁配。`pool.skipped` 只用于建池诊断，不参与叶结果。编码保留浮点 bit、序列顺序和 map 实际遍历顺序；layout 与 atom 首次精确编码后 intern，以完整 bytes 相等判定，并保活 Arc 防止地址复用或 `Arc::make_mut` 原地修改。含动态贸易 tagged 或制造 count-scaled 注入的 layout 额外按 Arc 实例分离，避免未经证明的 map clone/insert 求和顺序等价合并。

`search_trade_triples_filtered`（普通贸易入口委托它）与 `search_manufacture_triples` 建立线程本地 scope；调用方可用公开的 `ensure_search_memo_scope` 包住多个搜索。只有最外层 guard 创建和释放状态，内层复用；正常返回、错误及展开均释放该请求的状态，guard 不可跨线程，Rayon 组合任务不继承 scope。memo 独立于 perf 开关，不跨独立请求共享。scope 已接入完整排班、独立 assignment 和 CLI plan/profile 请求边界；两个独立公开搜索只有被外层 scope 包住时才共享结果。

成功 live 与 Bake 报告按完整内存 Clone 保存；命中保持 best/top、breakdown、combinations/evaluated 等原值，仅重新记录 elapsed。命中不新增实际 solver 调用或 Bake hit，也不伪造 generated/candidate/accepted 工作计数。perf 另累计 `memo_hits`、`memo_misses` 和 `memo_key_prepare_ms`；不具备缓存资格或编码失败不计 hit/miss，键准备耗时仅在叶入口记录一次，和原 prepare 阶段分开，诊断 JSON 仍在原统计输出边界生成。

本阶段只完成 fmt 与 workspace offline check；编译不能证明运行等价或性能收益，结果差分和运行验收集中在全部优化完成后。

## Bake

`bake_catalogs` 用 live solver 生成 schema v17 catalog 和 input fingerprint；贸易行以 `final_efficiency` 作为 `sort_efficiency`，校验按 `trade_final_efficiency` 检查排序键。`validate_baked_catalog` 检查 schema、输入 hash、row 集、combo table hash 和签名集合。贸易合法签名固定为 level1/2/3 赤金及 level3 源石：空 lv1/lv2 源石由 solver 的订单等级 hard constraint 不产生，故校验正好期望这四个 signature；manufacture 仍为 3 级 × 3 配方共 9 个。行保留贸易 total_efficiency、制造单位与每日产出；v17 以完整运行时身份索引行，不再把同名状态合并。查询沿 live 子池相对顺序枚举并使用制造仓库比较键，未表示的完整状态或非 canonical 顺序回退。`bake verify` 对存储行全量重算并比较完整命中，另做确定性子集查询差分；本轮运行验收集中在全部优化完成后。旧 catalog 因 schema 不匹配需重建。`data/baked/` 是首次运行生成的可丢弃数据，已加入 `.gitignore`；历史跟踪和旧 `aic-baked-save/` 已移除，不以仓库里的旧表作为当前构建的有效依据。

v17 的 `operator_states` 保存姓名、entry 的精英阶段、有序 buff ID 与标签；`operator_count` 统计状态数，`operator_names` 可以出现重复姓名。全表池从全部实例的贸易/制造设施绑定取姓名，用 E0/E1 tier_0、低星等级解锁以及 E1/E2 tier_up 的五类代表练度逐姓名走真实 pool builder，按完整身份去重；制造所有配方用同一完整变体池。此处扩大的是预计算存储域，生产池准入、standalone 策略和候选集合不变。组合只允许互异姓名；全表 canonical 顺序沿真实 pool 的效率提示降序、姓名升序，再以完整身份确定同名变体次序。行记录该顺序解算的数值；贸易赤金链 helper 重排时，行索引记录实际执行顺序，不能把同一状态集合默认视作可交换输入。

生成尝试三级 × 两种贸易订单，其中 lv1/lv2 源石实际为空；空行块不进入索引，完整表仍为四个贸易与九个制造 signature。生成改为固定组合迭代器 `par_bridge`，每 signature 完整排序后独立落盘并释放行块，再按原顺序拼接 schema17 header/行；流式 hash 保留整段 bytes 的长度前缀与 `DefaultHasher` 合同，临时块由 RAII 清理。

冷加载以完整身份建立 state→entry 索引，核对 canonical 状态集合；对同一打开 File 分块 hash、rewind 后流式读取 metadata 和单行。两域每个 signature 的存储行集合必须与同名互异且满足实际 legality 的完整合法 universe 相同，用 checked colex rank bitset 枚举清位核对缺行、多行及数量，避免同时保留 expected/actual 两套 BTreeSet。仍按 canonical 顺序重建有序 IDs 并核对实际贸易链重排；signature 的 OR 位图只做 verify 样本域快筛，不证明完整组合覆盖。

常驻表按 signature 保存 typed compact 行、有序 `u16` 状态 IDs、checked `u32` rank/规则字典，全部原始 `i32`/`f64` 保留；不逐行常驻姓名、mask、facility。贸易 96B/制造 48B 仅静态类型大小估计，不能代表 RSS。热查询只物化选中行的完整 owned 命中，不做 I/O；完整贸易 API 保留 signature+offset 标识与原签名，按需还原 owned 行及执行顺序，显式请求全行 Vec 仍有分配成本。全量 verify 按 offset 从同一 File 每批读取 4096 行，保留全部 Disk 字段/live 对账，并比较实际 compact 物化命中；320 查询差分覆盖不减少。内存改动已 release 编译、静态审查及完整 29,783,417 行/3,292 兼容查询零差异对账，2,148 查询按原门禁跳过；最终实现与实测已完成。第一轮轮换包包含选人/MAA/失败集合变化，历史差异保留；后续固定旧语义消融的152盒及full-e2两入口已确认成员/产出/rotation/MAA零差异，仅296叶为输入绝对路径标签，原same5/changed147保留，覆盖内纯性能归因可关闭。消融完整表3,435兼容查询零差异、2,005跳过；不推广任意上下文或独立Required+peer-reset，也不证明热性能收益。最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。 冷路径生命周期算法本轮未改，覆盖限制随最终包报告，不另开重生成或性能测量任务。

热查询要求每个真人只出现一次，完整状态在相应设施 canonical 池中，且输入保留该池相对顺序。生产 signature 查找使用固定 13 个可空槽：贸易 level1/2/3 赤金及 level3 源石占四槽，制造三级 × 三配方占九槽；先严格核对 level 1–3 与 capacity 等于等级，赤金另核对实际金线数等于冷加载基准，源石沿原 key 不限制金线数。冷加载按每个 index 首行恢复原字符串 key 并完整核对、拒绝重复槽；磁盘 v17、manifest 与逐行验证仍保留原字符串协议，热查询不再格式化 signature 或查字符串 HashMap。每个 signature 建立长度 `C(状态数, 容量)` 的 dense colex rank 数组，未存储组合用 sentinel 表示；容量只支持 1–3，组合数、rank 与数组分配均 checked，溢出或分配失败给出可读原因。查询沿 live 的原组合字典序枚举，固定三槽仅复制状态 ID 后排序计算 rank，不改变解算或输出顺序；直接 lookup 取代扫描和排序整张表。最大堆只保留 `top_k.max(1)` 个轻量候选，按贸易效率与原枚举次序、制造效率/仓库/组合姓名次序比较，最后物化 best/top。`evaluated` 仍统计全部通过合法性和必选过滤的组合，`combinations` 保留调用方枚举口径；贸易零命中 miss 回 live，制造保留成功空报告。

E0/E1 状态、full_pool 和有限班次可命中，不再固定 E2、24 心情或 24 小时；心情必须有限且大于零。实际技能中读取 Mood/OwnerMoodGap 或相关条件的池、制造 peer reset 继续 live，包括制造没有指定班次的情形。完整 LayoutContext 必须与 search baseline 相同，含 active_source_buffs；所有可参与计算的浮点值还核对有限性与 bit，静态注入 family map 核对实际遍历顺序和两域聚合结果，动态贸易注入与制造 count-scaled 注入回 live。传入 SkillTable 与 runtime 标准表的原始 JSON 文本逐字节匹配，结果按加载身份缓存；每个 entry 的预编译 atom 还按完整 seq、sort_key、atom 结构及浮点文本比较并缓存，持有 Arc 防止指针复用。制造 `None` 采用已匹配表的正常补编译语义，显式 `Some`（含空）必须与标准状态一致；贸易 legacy 空编译数组即使 live 可补编译，也会保守 miss。该临时 Debug 浮点比较只作表内门禁，不是持久化格式或通用 memo key。

确定性查询样本每个真人最多抽一个状态，并保留原池相对顺序；每批 320 个样本覆盖低阶变体、standalone/full_pool、top-K/必选及有限心情/班次。只对上述已明确不支持的边界计 `BakeVerifyReport.skipped_queries`，CLI 显示该计数；best、top、evaluated、combinations 或实际错误不一致都计差分失败。此阶段完成 fmt 与 release offline 编译，行/查询差分和最终运行验收集中在全部优化完成后；编译不能证明运行等价或性能收益。

Baked 行持久化单位贸易产出、单位赤金消耗、单位特殊赤金产出和单位源石消耗。命中时分别按实时路径用单位值 × `total_efficiency` 复原日产贸易与源石产出；特殊赤金产出再乘 `GSL_GOLD_UNIT_SCALE`，避免把普通订单赤金消耗误报为特殊产金。该恢复不改变候选集合、比较键或排序。

`BakeMode::Auto` 对不兼容查询回 live；首次兼容查询（或 `warm_runtime_baked_table`）加载缺失/失效表时，用显式 `Disabled` 的 live 路径生成完整 catalog 并做核心全量行校验与查询差分。该过程不调用 CLI 的 `run_all_regressions`。serve 启动阶段直接同步调用核心 warm，独立 trace 记录加载或生成；完成或失败后仍进入原 ready 流程，失败继续 live，不创建后台生成线程。生成期间 RAII 标记禁止递归自读；同进程初始化与失败原因由 `OnceLock` 缓存，失败只在 stderr 简短报告一次后回 live。`Disabled` 禁用；`Required` 只加载有效表、对任何 miss 失败，自身不生成。可执行文件 hash（含读取失败）在进程内只计算一次，继续保留输入文件指纹；进程已加载成功的内存表不会随运行中数据替换自动重载，新加载和显式校验仍会验证输入指纹。生成物不可手工编辑为规则。

生成写入 `baked/gen-<unique-id>/`，全部核心校验通过后冻结该目录及文件，再在短发布锁下复核目标，把临时指针文件原子 rename 成 `baked/CURRENT`。既有非空 `baked/` 和旧 generation 都不被原地替换。并发输方发现目标已有效就丢弃自己的 generation；锁不可取时不等待，尝试加载已完成的胜方，否则本进程回 live。失败/不可写不改变求解结果，不发布 limited catalog。已发布的 generation 保留给读取者；进程异常终止可能留下未引用 generation 或发布锁，仍可读取有效 CURRENT，未有有效表时保守回 live。

runtime、CLI 默认输出和 debug 的 manifest 发现共用 `default_baked_out_dir`，所有校验/verify/完整贸易 API 由同一 owner 解析 CURRENT。CURRENT 仅接受一个子目录名，并校验实际目录未越过根目录；没有 CURRENT 时兼容旧平铺 catalog。`ARKNIGHTS_INFRA_BAKED_DIR` 设置时直接指定 Bake 目录，即使目录尚未生成也不回退到其他目录；只影响 Bake 默认目录，不改变运行时输入文件发现或 report 标签。CLI 显式 `--out` 仍采用给定目录。未设置该专用变量时，目录发现先按环境变量 `ARKNIGHTS_INFRA_DATA_DIR`、exe 同目录 `data`、exe 父目录 `data`、当前工作目录 `data` 的顺序选已有 CURRENT/manifest；全部缺失时取第一根。这保留既有默认输出规则：仓库中有旧 `data/baked/manifest.json` 时仍选它；全部删除且无环境变量时 `target/release/infra-cli` 首次生成位置是 `target/release/data/baked`，设置环境变量且无其他已有表时生成到该变量的 `baked` 子目录。`ARKNIGHTS_INFRA_DATA_DIR` 并非对既有 Bake 目录的强制覆盖。CLI 完整生成走同一核心发布 API，limited 生成只允许写独立非 CURRENT 目录，之后原有完整校验仍会拒绝该 limited 表。

黑键—巫恋产品禁配只在读取已验证完整的贸易 Bake 表时过滤，先排除非法行再取前 K 条；它本身不造成 `Required` miss。其他未表示的同站禁配仍由 `Auto` 回 live、`Required` 报错。catalog 的可执行文件指纹失效时，`Auto` 首次尝试重建，`Required` 照常 miss，不能拿旧表证明命中。

核验：`bake validate`、`bake verify`、`infra-cli verify`，并记录输入、组合数、evaluated 和命中模式。候选/剪枝/Bake 改动还要运行最终 assignment invariant gate。

请求scope覆盖整个 `schedule_timed_rotation_request`、ABC候选共同入口、独立assignment入口以及CLI单次plan/profile run；嵌套搜索只复用外层scope。因此N/A/B候选、控制中枢重建和各房间搜索可复用完整输入一致的叶报告，请求结束即释放；不传播到Rayon组合评估线程，不跨请求缓存。此阶段仅编译，真实命中率及性能留最终统一包。

## Bake 诊断计数

query miss 只在两域 `try_baked_*` 入口按固定枚举累计一次；compatibility 一次返回 typed 原因，Required 才延迟格式化原错误，Auto 不构造 miss 诊断字符串。制造搜索不再在此入口之前单独跳过 peer-reset 池，由同一兼容门禁记原因并保证 Required 严格报错。内部 report helper 的 verify 调用、load 和 warm 不再计 query miss；加载中的 `generating` 与缓存失效/不可用的 `unavailable` 分开。rank sentinel 跳过和制造成功空报告不计 miss，rank/row 越界或物化等原 Err 继续传播，另记 `query_errors`，不能改成成功回退。

`metrics.bake.trade` / `manufacture` 输出非零类别：`forbidden_pairs`、`override`、`predicate`、`invalid_mood`、`invalid_shift`、`layout`、`mood_sensitive`、`ramp`、`peer_reset`、`generating`、`unavailable`、`skill_table`、`duplicate_names`、`state_absent`、`facility_state_absent`、`noncanonical_order`、`trade_atoms`、`manufacture_atoms`、`signature_absent`、`capacity`、`heap_unavailable`、`trade_incomplete`、`manufacture_incomplete`、`no_eligible_trade`。

制造有班长且池内技能包含 `AddEffRamp` 时，以 `ramp` 原因整体回 live；Required 严格报 miss。live连续积分实现用解析平均与阈值分段加权，心情区间不重启暖机；Some(h)使用真实时长，不round/clamp。首轮冻结记录保留，新冻结局部数值验收及指定P1/P2静态闭合完成；1122full含72预期非有限错误、1050search中798获准精确一致、252既有>=12h负心情扣减政策排除，不能认证所有搜索组合；最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。 槐琥分桶修正为全部EffVar直接贡献后读取（order10→30，五ramp order9保持），普通段按阈值分段积分；极短段保留真实权重，暖机解析平均、非线性按原解释器可表示点求值，保留f64分辨率和舍入边界。无班长默认 20 小时，固定 Bake 行须按新连续积分口径重新生成与全量校验；带班长且含暖机的查询差分明确计 skipped，不计 checked。此门禁不删候选，也不把 20 小时表当作新班长结果。

冷阶段 `metrics.bake.cold` 按 `init`、`generate`、`validate`、`verify`、`index`、`publish` 保存 calls/errors/elapsed_ms；init 只覆盖真正 OnceLock 初始化，缓存访问不冒充冷加载。实际重复验证照次数记；父子阶段 inclusive，不能相加作总耗时。CLI bake 在动作体包 trace，保留核心全量校验与既有 CLI 回归失败；serve 的启动 warm 单独输出 trace。

所有计数在当前 trace 的固定数值数组累积，没有 trace 时不统计。JSON 与标签只在 trace 结束生成；schema 2 的 operation 和 Bake 数值指标省略零值，缺字段按零解释，顶层 event/schema 与阶段结构保留。本阶段仅格式化、workspace 离线编译和文档/源差异检查，未运行 solver、Bake、性能或对拍。

## 自动建议候选

自动模式的候选使用真实站点成员快照检查协同准入，后续候选也不能激活此前房间的禁止依赖。候选收益窗口由主备输入H给出；完整成员预算复用心情owner。自动取消≥12小时负心情差排除，普通定时既有政策保留。模式禁令为 hard constraint，现有选人/顺序/Top-K仍属 heuristic，窗口为 policy，不声称完整或全局最优。新增模式、成员和共享主力快照进入请求memo身份；自动模式两域直接live，不读固定班时Bake，不放宽Bake兼容门禁。定时默认输入仍走原搜索与Bake策略。

auto候选房身份进入memo输入；备态候选还检查与固定主态其他房可共存的关系，不把同房主备混合。每房搜索使用 `CandidateAdmission` 借用固定站点，仅构造当前候选房，按需复用候选无关的另一态与共享宿舍检查；宿舍候选变化仍独立重查。成员改变可能激活外房依赖，因此完整当前态和原有房对判定保留，资源来源仍由实际解释器按需求值。LayoutContext的两态站位共享只读所有权，memo与序列化按内容，发电逐房提交时写时复制。终态枚举实际不同的房状态验证独立切换禁令，只对空非工作房的纯H差异消除等价分支。同一次终态检查按真实房态、中枢态和必要深海人数复用预算，缓存不跨调用，逐叶协同检查保留。以上是等价复用及safe reduction，不改候选或排序。

协同准入读取中枢实际资源写入、发电虚拟产出与跨设施global执行器的写入事实，同一成员同一资源净值为零不登记；全局conversion只沿实际执行边传播来源并登记在岗转换者。蓝图初始pool保留，因此预置碎片经一级絮雨转换仍构成外站成员依赖。房内状态转换不发布为其他站的供给。真实站点等级、初始pool、静态场景及主力快照进入memo身份。来源在同一次准入中按需读取，实际资源消费者才触发原解释器。

发电搜索接调用方本轮空房列表并随选入更新真实房快照；夜烟影子探测在auto使用清空后的试排副本，贸易与制造分别绑定实际目标房。

普通两/三班及自动的Plan早期贸易gate、制造receiver比较、完整制造/发电与普通基线都读取 `RuleCompileContext.shift_hours`。排班options时长写入seed后传入编译上下文；无排班时长的独立Plan入口保留24h默认。三班Peak沿既有累计窗口，γ两半使用各自输入时长，不改变候选截断或比较目标。

自动宿舍1＋1的受益者由原设施搜索直接决定：临时实际挂件环境下清空该房预选成员，不设must-include；赢家包含受益者才提交原Plan成员与依赖。制造使用已有full_pool，贸易保留有序role链并使普通full_pool输入贯通上层筛选，发电复用单站assignment搜索。该路径不再补排全制造/全发电或重搜无挂件基线；普通定时competitive政策保留。规则及alternative仍有序取首个可行方案，属于既有heuristic，不声称跨alternative或全布局最优。
