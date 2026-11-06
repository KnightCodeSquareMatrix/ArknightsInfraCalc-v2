# 候选搜索与 Bake

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/SCORING_MODEL.md; docs/PERFORMANCE_ENGINEERING.md
> 机制依赖：docs/知识库引用.md；vault:docs/1-基础设定/设施与进驻/基建总览.md；vault:docs/3-组合库/组合注册表.md
> 摘要：定位候选池、Plan 门禁、域搜索、排序与 Bake 回退的完整链路。
> 代码范围：`crates/infra-core/src/{pool,search,bake.rs,candidate.rs}`; `crates/infra-cli/src/commands/{bake,profile}.rs`; `data/baked/`
> 代码锚点：`crates/infra-core/src/pool/base.rs::{build_roster_pool,filter_pool}`; `crates/infra-core/src/search/{trade,manufacture,control,power,trade_allocation}.rs`; `crates/infra-core/src/bake.rs::{bake_catalogs,load_baked_catalog,try_baked_trade_search}`
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

`search_trade_triples`、`search_manufacture_triples`、`search_control_combos`、`search_power_assignment` 各自负责单域入口；手动Peak和γ自有房间的贸易联合选择使用`search_trade_allocation`，按金贸/源石词典序比较完整房间组合，不截断每房top-K。它复用单房真实结算，在caller给定的共享状态内缓存无变化组合（含失败），再用各房独立最优乐观上界剪枝；局部缓存只存在于本次联合调用，不进入单房report memo或Bake。支持上下文、Plan绑定、已建模状态及暂缓范围见[贸易求解](贸易求解.md#搜索与跨站)。遇到“候选没被考虑”，按 pool skip → Plan claim/exclusion → used → capacity → domain filter → comparator 排查。

`ControlFillPolicy::LayeredFill` 当前走中枢专用顺序入口：先满足适用候选组，再按贸易、制造、训练（没有专精上下文时保持 `Unsupported`）、办公室、会客室、心情和普通补位逐位消费“当前已选 + 当前候选”。前序席位锁定后，后序候选不能用匿名加权分数替换它。`build_control_pool_with_fillers` 只增加真实没有设施技能的干员；带未建模 buff 的干员留在 `skipped`，不会被清空技能后重新加入。

该顺序入口的贸易/制造区只读取冻结 `ControlEffectLedger` 的静态全局 key；按人数缩放、当前房间、deferred producer 和动态 projection 留给后续 resolve，不得提前成为中枢区 winner。调用方提供的结构eligibility 在候选组、每个效果区、统一心情区和普通补位每次形成满容量候选时都会检查。候选组逐组贪心仍是具名 policy，不能把 `NO_CANDIDATE_UNDER_POLICY` 或候选耗尽解释成 canonical `INFEASIBLE`。

## 请求内搜索 memo

live 执行成本优化保留原候选与报告：制造同伴兼容特征按当前子池预备，组合检查与完整物化共用同一规则；所有组合评价后按效率、仓库、姓名和原枚举下标选出前 K，只排序/物化该前缀，零 K 保留 best。中枢顺序入口对同一 trial 只结算一次，结果同时用于合法性和候选组/效果区排名。贸易整体互斥复用已识别的房间特征，独立互斥 API 仍保留。这些属于 `safe_reduction`，不改变具名 policy、计数、决胜或 Bake 支持域。

贸易单订单和制造单配方叶入口在各自过滤、组合准备之前查询请求内 memo。这属于完整输入精确相等的 `safe reduction`：复用成功 report，不更改候选、必选、禁配、排序、top-K 或组合计数；`Stations` / `Lines` 包装器仍分别调用叶搜索并自行组合报告。叶入口的错误不缓存，`BakeMode::Required` 和带贸易 hit predicate 的查询不缓存；键编码失败保守执行原搜索。

键覆盖不可变 SkillTable identity、有序完整 pool entry（含练度、buff、标签、预编译 atom、hint、mechanic/delegate、tier）、完整 LayoutContext 及其私有 global 数据、全部 options 和叶订单/配方、贸易 must/完整 override/禁配。`pool.skipped` 只用于建池诊断，不参与叶结果。编码保留浮点 bit、序列顺序和 map 实际遍历顺序；layout 与 atom 首次精确编码后 intern，以完整 bytes 相等判定，并保活 Arc 防止地址复用或 `Arc::make_mut` 原地修改。含动态贸易 tagged 或制造 count-scaled 注入的 layout 额外按 Arc 实例分离，避免未经证明的 map clone/insert 求和顺序等价合并。

`search_trade_triples_filtered`（普通贸易入口委托它）与 `search_manufacture_triples` 建立线程本地 scope；调用方可用公开的 `ensure_search_memo_scope` 包住多个搜索。只有最外层 guard 创建和释放状态，内层复用；正常返回、错误及展开均释放该请求的状态，guard 不可跨线程，Rayon 组合任务不继承 scope。memo 独立于 perf 开关，不跨独立请求共享。scope 已接入完整排班、独立 assignment 和 CLI plan/profile 请求边界；两个独立公开搜索只有被外层 scope 包住时才共享结果。

成功 live 与 Bake 报告按完整内存 Clone 保存；命中保持 best/top、breakdown、combinations/evaluated 等原值，仅重新记录 elapsed。命中不新增实际 solver 调用或 Bake hit，也不伪造 generated/candidate/accepted 工作计数。perf 另累计 `memo_hits`、`memo_misses` 和 `memo_key_prepare_ms`；不具备缓存资格或编码失败不计 hit/miss，键准备耗时仅在叶入口记录一次，和原 prepare 阶段分开，诊断 JSON 仍在原统计输出边界生成。

本阶段只完成 fmt 与 workspace offline check；编译不能证明运行等价或性能收益，结果差分和运行验收集中在全部优化完成后。

## Bake

2026-11-02 用户裁决：删除全部 Bake 校验及自动机制回归。生成后直接原子发布；加载仅解码 compact 行并建立查询索引，不检查 schema、exe/输入/文件指纹、签名集合、字段、排序、合法全集或执行顺序，不做逐行/抽样 live 对账。bake validate/verify 和对应 API 已删除。磁盘 v17 布局保留，指纹槽位不再填写。数据或计算实现更新后由调用方显式重建表。此为缓存信任 policy，取消运行时完整性与新鲜度保证；热查询适用条件、真实搜索禁配和必要的解码/类型转换/分配/索引错误仍保留。

v17 的 `operator_states` 保存姓名、entry 的精英阶段、有序 buff ID 与标签；`operator_count` 统计状态数，`operator_names` 可以出现重复姓名。全表池从全部实例的贸易/制造设施绑定取姓名，用 E0/E1 tier_0、低星等级解锁以及 E1/E2 tier_up 的五类代表练度逐姓名走真实 pool builder，按完整身份去重；制造所有配方用同一完整变体池。此处扩大的是预计算存储域，生产池准入、standalone 策略和候选集合不变。组合只允许互异姓名；全表 canonical 顺序沿真实 pool 的效率提示降序、姓名升序，再以完整身份确定同名变体次序。行记录该顺序解算的数值；贸易赤金链 helper 重排时，行索引记录实际执行顺序，不能把同一状态集合默认视作可交换输入。

生成尝试三级 × 两种贸易订单，其中 lv1/lv2 源石实际为空；空行块不进入索引，完整表仍为四个贸易与九个制造 signature。生成使用固定组合迭代器按批次评价，每批排序后立即落盘，外部归并得到signature行块，再按原顺序拼接 schema17 header/行；临时块由 RAII 清理，拼接后不计算文件 hash。

冷加载直接流式解码 metadata 和每行，保留 typed compact 数值、有序状态 IDs 和规则字典；不构造合法 universe 位图，不重建每个组合，不枚举合法组合全集。dense colex rank 索引用于热查询，只物化选中行。贸易 96B/制造 48B 仅为类型大小，不能代表 RSS；显式请求全行 Vec 仍有分配成本。

<!-- knowledge:historical -->
### 内存与旧暖机预算的历史验收

下述旧预算失败为当轮快照；当前制造≥12h排除和后续预算拒绝已删除，不能据此恢复。

内存改动已 release 编译、静态审查及完整 29,783,417 行/3,292 兼容查询零差异对账，2,148 查询按原门禁跳过；最终实现与实测已完成。第一轮轮换包包含选人/MAA/失败集合变化，历史差异保留；后续固定旧语义消融的152盒及full-e2两入口已确认成员/产出/rotation/MAA零差异，仅296叶为输入绝对路径标签，原same5/changed147保留，覆盖内纯性能归因可关闭。消融完整表3,435兼容查询零差异、2,005跳过；不推广任意上下文或独立Required+peer-reset，也不证明热性能收益。最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。 冷路径生命周期算法本轮未改，覆盖限制随最终包报告，不另开重生成或性能测量任务。

### 当前热查询与兼容门禁


热查询要求每个真人只出现一次，完整状态在相应设施 canonical 池中，且输入保留该池相对顺序。生产 signature 查找使用固定 13 个可空槽：贸易 level1/2/3 赤金及 level3 源石占四槽，制造三级 × 三配方占九槽；先严格核对 level 1–3 与 capacity 等于等级，赤金另核对实际金线数等于冷加载基准，源石沿原 key 不限制金线数。冷加载按每个 index 首行建立 signature 槽；磁盘 v17 与 manifest 保留原字符串协议，热查询不再格式化 signature 或查字符串 HashMap。每个 signature 建立长度 `C(状态数, 容量)` 的 dense colex rank 数组，未存储组合用 sentinel 表示；容量只支持 1–3，组合数、rank 与数组分配均 checked，溢出或分配失败给出可读原因。查询沿 live 的原组合字典序枚举，固定三槽仅复制状态 ID 后排序计算 rank，不改变解算或输出顺序；直接 lookup 取代扫描和排序整张表。最大堆只保留 `top_k.max(1)` 个轻量候选，按贸易效率与原枚举次序、制造效率/仓库/组合姓名次序比较，最后物化 best/top。`evaluated` 仍统计全部通过合法性和必选过滤的组合，`combinations` 保留调用方枚举口径；贸易零命中 miss 回 live，制造保留成功空报告。

E0/E1 状态、full_pool 和有限班次可命中，不再固定 E2、24 心情或 24 小时；心情必须有限且大于零。实际技能中读取 Mood/OwnerMoodGap 或相关条件的池、制造 peer reset 继续 live，包括制造没有指定班次的情形。LayoutContext除调度选择字段auto_rotation/work_hours外必须与search baseline相同；有效评价窗口单独核对，时间相关制造ramp等不支持输入回live，含 active_source_buffs；所有可参与计算的浮点值还核对有限性与 bit，静态注入 family map 核对实际遍历顺序和两域聚合结果，动态贸易注入与制造 count-scaled 注入回 live。传入 SkillTable 与 runtime 标准表的原始 JSON 文本逐字节匹配，结果按加载身份缓存；每个 entry 的预编译 atom 还按完整 seq、sort_key、atom 结构及浮点文本比较并缓存，持有 Arc 防止指针复用。制造 `None` 采用已匹配表的正常补编译语义，显式 `Some`（含空）必须与标准状态一致；贸易 legacy 空编译数组即使 live 可补编译，也会保守 miss。该临时 Debug 浮点比较只作表内门禁，不是持久化格式或通用 memo key。


Baked 行持久化单位贸易产出、单位赤金消耗、单位特殊赤金产出和单位源石消耗。命中时分别按实时路径用单位值 × `total_efficiency` 复原日产贸易与源石产出；特殊赤金产出再乘 `GSL_GOLD_UNIT_SCALE`，避免把普通订单赤金消耗误报为特殊产金。该恢复不改变候选集合、比较键或排序。

`BakeMode::Auto` 对不兼容查询回 live；首次兼容查询或 warm 读取失败时，用 Disabled 的 live 路径生成并发布，不调用校验或机制回归。serve 同步 warm 后进入 ready，失败继续 live；RAII 防递归，OnceLock 缓存成功与失败。Required 不生成。进程不自动重载表；更新后显式重建。

生成写入 `baked/gen-<unique-id>/`，生成完成后冻结该目录及文件，再在短发布锁下切换 CURRENT，把临时指针文件原子 rename 成 `baked/CURRENT`。既有非空 `baked/` 和旧 generation 都不被原地替换。锁竞争输方发现已发布文件存在就丢弃自己的 generation；锁不可取时不等待，尝试加载已完成的胜方，否则本进程回 live。失败/不可写不改变求解结果，不发布 limited catalog。已发布的 generation 保留给读取者；进程异常终止可能留下未引用 generation 或发布锁，仍可读取有效 CURRENT，未有有效表时保守回 live。

runtime、CLI 默认输出和 debug 的 manifest 发现共用 `default_baked_out_dir`，加载与完整贸易 API 由同一 owner 解析 CURRENT。CURRENT 仅接受一个子目录名，并校验实际目录未越过根目录；没有 CURRENT 时兼容旧平铺 catalog。`ARKNIGHTS_INFRA_BAKED_DIR` 设置时直接指定 Bake 目录，即使目录尚未生成也不回退到其他目录；只影响 Bake 默认目录，不改变运行时输入文件发现或 report 标签。CLI 显式 `--out` 仍采用给定目录。未设置该专用变量时，目录发现先按环境变量 `ARKNIGHTS_INFRA_DATA_DIR`、exe 同目录 `data`、exe 父目录 `data`、当前工作目录 `data` 的顺序选已有 CURRENT/manifest；全部缺失时取第一根。这保留既有默认输出规则：仓库中有旧 `data/baked/manifest.json` 时仍选它；全部删除且无环境变量时 `target/release/infra-cli` 首次生成位置是 `target/release/data/baked`，设置环境变量且无其他已有表时生成到该变量的 `baked` 子目录。`ARKNIGHTS_INFRA_DATA_DIR` 并非对既有 Bake 目录的强制覆盖。CLI 完整生成走同一核心发布 API，limited 生成只允许写独立非 CURRENT 目录，调用方不得把 limited 目录配置为运行时完整表。

黑键—巫恋产品禁配只在读取贸易 Bake 表时过滤，先排除非法行再取前 K 条；它本身不造成 `Required` miss。其他未表示的同站禁配仍由 `Auto` 回 live、`Required` 报错。更新后需显式重建，不再检查可执行文件指纹。

核验：真实 live/Bake CLI 结果对比，并记录输入、组合数、evaluated 和命中模式。候选/剪枝/Bake 改动还要运行最终 assignment invariant gate。

请求scope覆盖整个 `schedule_timed_rotation_request`、ABC候选共同入口、独立assignment入口以及CLI单次plan/profile run；嵌套搜索只复用外层scope。因此N/A/B候选和各房间搜索可复用完整输入一致的叶报告，请求结束即释放；不传播到Rayon组合评估线程，不跨请求缓存。此阶段仅编译，真实命中率及性能留最终统一包。

## Bake 诊断计数

query miss 只在两域 `try_baked_*` 入口按固定枚举累计一次；compatibility 一次返回 typed 原因，Required 才延迟格式化原错误，Auto 不构造 miss 诊断字符串。制造搜索不再在此入口之前单独跳过 peer-reset 池，由同一兼容门禁记原因并保证 Required 严格报错。内部 report helper、load 和 warm 不再计 query miss；加载中的 `generating` 与缓存失效/不可用的 `unavailable` 分开。rank sentinel 跳过和制造成功空报告不计 miss，rank/row 越界或物化等原 Err 继续传播，另记 `query_errors`，不能改成成功回退。

`metrics.bake.trade` / `manufacture` 输出非零类别：`forbidden_pairs`、`override`、`predicate`、`invalid_mood`、`invalid_shift`、`layout`、`mood_sensitive`、`ramp`、`peer_reset`、`generating`、`unavailable`、`skill_table`、`duplicate_names`、`state_absent`、`facility_state_absent`、`noncanonical_order`、`trade_atoms`、`manufacture_atoms`、`signature_absent`、`capacity`、`heap_unavailable`、`trade_incomplete`、`manufacture_incomplete`、`no_eligible_trade`。

制造有班长且池内技能包含 `AddEffRamp` 时，以 `ramp` 原因整体回 live；Required 严格报 miss。live连续积分实现用解析平均与阈值分段加权，心情区间不重启暖机；Some(h)使用真实时长，不round/clamp。

<!-- knowledge:historical -->
### 暖机局部验证的旧政策快照

下述数量与旧预算失败属于连续暖机实施时的历史证据，制造≥12h排除与心情预算拒绝已由2026-10-31裁决删除，不能解释当前行为。首轮冻结记录保留，新冻结局部数值验收及指定P1/P2静态闭合完成；1122full含72预期非有限错误、1050search中798获准精确一致、252既有>=12h负心情扣减政策排除，不能认证所有搜索组合；最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。

### 当前暖机与诊断计时

 槐琥分桶修正为全部EffVar直接贡献后读取（order10→30，五ramp order9保持），普通段按阈值分段积分；极短段保留真实权重，暖机解析平均、非线性按原解释器可表示点求值，保留f64分辨率和舍入边界。无班长默认 20 小时，固定 Bake 行须按新连续积分口径显式重新生成；带班长且含暖机的热查询仍回 live。此门禁不删候选，也不把 20 小时表当作新班长结果。

冷阶段 metrics.bake.cold 只保留 init、generate、index、publish；validate/verify 阶段已删除。父子耗时 inclusive，不能相加作为总耗时。CLI bake 与 serve warm 保留 trace。

所有计数在当前 trace 的固定数值数组累积，没有 trace 时不统计。JSON 与标签只在 trace 结束生成；schema 2 的 operation 和 Bake 数值指标省略零值，缺字段按零解释，顶层 event/schema 与阶段结构保留。本阶段仅格式化、workspace 离线编译和文档/源差异检查，未运行 solver、Bake、性能或对拍。

## 自动轮换候选

2026-11-05补齐非中枢正常技能与体系候选初筛。单班/主备用已有owner窗口，ABC用已有Peak窗口；各池及锚点注入消费冻结的设施资格，后续组合不重筛。已有中枢检查和Fallback例外按用户要求保持，本次为候选范围policy修复，不改排序、Bake或全局最优保证。

自动模式在Plan入口删除禁用组合，整体跳过兼容registry与动态producer前缀竞争；允许域沿普通设施搜索、局部禁配、required和used。已删除CandidateAdmission、全站技能presence、资源来源复演及任意主备混搭穷举。按用户2026-10-30删减裁决，不再保证零散成员/任意混搭没有偶发禁用协同，此项属于approximation；没有新top-K或有限精查策略。

所有排班模式保留最初名单ETA试算；2026-11-05已补齐非中枢正常技能与体系候选，已有中枢检查和Fallback例外保持；搜索不再做本房、双中枢或固定主力预算准入，memo移除这些状态，仅保留数值收益layout、模式与H。制造≥12小时负心情差身份排除已删除，真实积分与暖机保留。自动与定时均可尝试Bake，依真实结算状态及有效窗口决定适用性。

终态核对实际结构、Plan、显式挂件归属与快照；工时不足仅诊断，显式 gate 核对提示准确性，不恢复每房两名单×两中枢工作预算拒绝或全站协同枚举。详见[自动轮换](../自动轮换.md)。
制造体系试排直接调用原实时制造搜索，保留请求memo与实际收益上下文。额外条件响应表已按2026-10-31真实动态上下文测速结果删除：其复用收益不足以抵消全量生成和排序成本。候选、排序和保证等级保持，取舍证据见[搜索性能与Bake边界](../PERFORMANCE_ENGINEERING.md#制造体系试排的缓存取舍2026-10-31)。

### 流式生成与外部排序（2026-11-02）

完整生成每批最多32,768个组合，Rayon仅评价当前批次；按原效率降序、制造仓库降序、有序状态ID升序排序后立即序列化。16路有界归并生成分层run，最终按原signature顺序拼接v17文件；不全量收集signature行。临时run由RAII释放，归并每路只持有一行及256KiB读取缓冲。候选、数值、执行顺序及排序不变，类别为safe reduction；limited入口仍用原有有界前缀堆。

2026-11-02按用户“先暂时禁用”的裁决，Core与CLI运行时Bake开关均关闭：普通搜索、定时/自动排班及serve不加载、不预热、不查表，直接使用live搜索。保留 --no-bake 参数兼容既有调用，显式 bake all/trade/manufacture 生成入口不受影响。当前CLI没有运行时启用参数；以后接通生产动态布局时再恢复。bench仍输出perf trace，可检查live_calls与冷阶段；不能仅凭总时间推断命中。

bench --runs N 在同一进程顺序执行N次相同输入，各次独立trace与请求memo；当前运行时Bake关闭，各轮均走live，不加载或复用Bake表。输出文件请求会逐轮写入同一路径，最终保留最后一轮。

### 生产支持域与实测（2026-11-02）

全量30,337,349行外部排序与旧payload逐字节相同，生成192.505s，GNU time峰值RSS65.35MiB、无swap。运行时仍加载完整compact表及dense索引：兼容bench首轮init26.08s，RSS约2.71GiB；5次热请求搜索中位开启0.961ms/关闭3.644ms，真实命中2次贸易+1次制造，结果相同。

前一轮三班38个Box开启/关闭零命中且完整产物相同，属于修复前证据。2026-11-02用户纠正“自动排版不需要绕过bake，这是agent的误解”：已移除两域搜索入口的自动禁用；布局兼容比较只排除选择评价窗口的auto_rotation/work_hours，真实名单、global注入等机制状态仍精确比较。有效评价窗口沿LayoutContext::evaluation_hours取值，自动制造也按该窗口拒绝固定行不能表达的ramp；mood和peer-reset等原条件保持。这是cache/fallback支持域扩展；等价依据限于调度字段不参与冻结响应且原动态依赖门禁仍成立，不改变候选、目标或机制；不是允许任意动态布局读固定表。serve预热不保证后续请求能命中。具体RIIC-Web原生客户端实测、自动CLI及验收见docs/PERFORMANCE_ENGINEERING.md后续段。


实际兼容自动正例已用release core入口验证：auto=true、work_hours=Some(12)、其余为表基准的静态制造查询Required命中，best/top及计数与live一致；加入暖机状态后Required报Ramp、Auto回live。因此自动模式可用兼容Bake行，真实布局状态失配仍回live。验证不证明任意自动布局支持。

本次禁用属于cache使用policy调整，恢复原live搜索，不改变候选、目标或完整性保证；显式生成及兼容性判断代码保留，供后续接通。
