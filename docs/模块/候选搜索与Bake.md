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

## Bake

`bake_catalogs` 用 live solver 生成 schema v17 catalog 和 input fingerprint；贸易行以 `final_efficiency` 作为 `sort_efficiency`，校验按 `trade_final_efficiency` 检查排序键。`validate_baked_catalog` 检查 schema、输入 hash、row 集、combo table hash 和签名集合。贸易合法签名固定为 level1/2/3 赤金及 level3 源石：空 lv1/lv2 源石由 solver 的订单等级 hard constraint 不产生，故校验正好期望这四个 signature；manufacture 仍为 3 级 × 3 配方共 9 个。行保留贸易 total_efficiency、制造单位与每日产出；v17 以完整运行时身份索引行，不再把同名状态合并。查询沿 live 子池相对顺序枚举并使用制造仓库比较键，未表示的完整状态或非 canonical 顺序回退。`bake verify` 对存储行全量重算并比较完整命中，另做确定性子集查询差分；本轮运行验收集中在全部优化完成后。旧 catalog 因 schema 不匹配需重建。`data/baked/` 是首次运行生成的可丢弃数据，已加入 `.gitignore`；历史跟踪和旧 `aic-baked-save/` 已移除，不以仓库里的旧表作为当前构建的有效依据。

v17 的 `operator_states` 保存姓名、entry 的精英阶段、有序 buff ID 与标签；`operator_count` 统计状态数，`operator_names` 可以出现重复姓名。全表池从全部实例的贸易/制造设施绑定取姓名，用 E0/E1 tier_0、低星等级解锁以及 E1/E2 tier_up 的五类代表练度逐姓名走真实 pool builder，按完整身份去重；制造所有配方用同一完整变体池。此处扩大的是预计算存储域，生产池准入、standalone 策略和候选集合不变。组合只允许互异姓名；全表 canonical 顺序沿真实 pool 的效率提示降序、姓名升序，再以完整身份确定同名变体次序。行记录该顺序解算的数值；贸易赤金链 helper 重排时，行索引记录实际执行顺序，不能把同一状态集合默认视作可交换输入。

冷加载以完整身份建立 state→entry 索引，核对 canonical 状态集合；两域每个 signature 的存储行集合必须与同名互异且满足实际 legality 的组合身份 tuple 完全相同，signature 的 OR 位图只做 verify 样本域快筛，不证明完整组合覆盖。行级 verify 和完整贸易 API 都按行的完整身份及执行顺序还原，避免逐行按姓名取首项。

热查询要求每个真人只出现一次，完整状态在相应设施 canonical 池中，且输入保留该池相对顺序。每个 signature 建立长度 `C(状态数, 容量)` 的 dense colex rank 数组，未存储组合用 sentinel 表示；容量只支持 1–3，组合数、rank 与数组分配均 checked，溢出或分配失败给出可读原因。查询沿 live 的原组合字典序枚举，固定三槽仅复制状态 ID 后排序计算 rank，不改变解算或输出顺序；直接 lookup 取代扫描和排序整张表。最大堆只保留 `top_k.max(1)` 个轻量候选，按贸易效率与原枚举次序、制造效率/仓库/组合姓名次序比较，最后物化 best/top。`evaluated` 仍统计全部通过合法性和必选过滤的组合，`combinations` 保留调用方枚举口径；贸易零命中 miss 回 live，制造保留成功空报告。

E0/E1 状态、full_pool 和有限班次可命中，不再固定 E2、24 心情或 24 小时；心情必须有限且大于零。实际技能中读取 Mood/OwnerMoodGap 或相关条件的池、制造 peer reset 继续 live，包括制造没有指定班次的情形。完整 LayoutContext 必须与 search baseline 相同，含 active_source_buffs；所有可参与计算的浮点值还核对有限性与 bit，静态注入 family map 核对实际遍历顺序和两域聚合结果，动态贸易注入与制造 count-scaled 注入回 live。传入 SkillTable 与 runtime 标准表的原始 JSON 文本逐字节匹配，结果按加载身份缓存；每个 entry 的预编译 atom 还按完整 seq、sort_key、atom 结构及浮点文本比较并缓存，持有 Arc 防止指针复用。制造 `None` 采用已匹配表的正常补编译语义，显式 `Some`（含空）必须与标准状态一致；贸易 legacy 空编译数组即使 live 可补编译，也会保守 miss。该临时 Debug 浮点比较只作表内门禁，不是持久化格式或通用 memo key。

确定性查询样本每个真人最多抽一个状态，并保留原池相对顺序；每批 320 个样本覆盖低阶变体、standalone/full_pool、top-K/必选及有限心情/班次。只对上述已明确不支持的边界计 `BakeVerifyReport.skipped_queries`，CLI 显示该计数；best、top、evaluated、combinations 或实际错误不一致都计差分失败。此阶段完成 fmt 与 release offline 编译，行/查询差分和最终运行验收集中在全部优化完成后；编译不能证明运行等价或性能收益。

Baked 行持久化单位贸易产出、单位赤金消耗、单位特殊赤金产出和单位源石消耗。命中时分别按实时路径用单位值 × `total_efficiency` 复原日产贸易与源石产出；特殊赤金产出再乘 `GSL_GOLD_UNIT_SCALE`，避免把普通订单赤金消耗误报为特殊产金。该恢复不改变候选集合、比较键或排序。

`BakeMode::Auto` 对不兼容查询回 live；首次兼容查询（或 `warm_runtime_baked_table`）加载缺失/失效表时，用显式 `Disabled` 的 live 路径生成完整 catalog 并做核心全量行校验与查询差分。该过程不调用 CLI 的 `run_all_regressions`。生成期间 RAII 标记禁止递归自读；同进程初始化与失败原因由 `OnceLock` 缓存，失败只在 stderr 简短报告一次后回 live。`Disabled` 禁用；`Required` 只加载有效表、对任何 miss 失败，自身不生成。可执行文件 hash（含读取失败）在进程内只计算一次，继续保留输入文件指纹；进程已加载成功的内存表不会随运行中数据替换自动重载，新加载和显式校验仍会验证输入指纹。生成物不可手工编辑为规则。

生成写入 `baked/gen-<unique-id>/`，全部核心校验通过后冻结该目录及文件，再在短发布锁下复核目标，把临时指针文件原子 rename 成 `baked/CURRENT`。既有非空 `baked/` 和旧 generation 都不被原地替换。并发输方发现目标已有效就丢弃自己的 generation；锁不可取时不等待，尝试加载已完成的胜方，否则本进程回 live。失败/不可写不改变求解结果，不发布 limited catalog。已发布的 generation 保留给读取者；进程异常终止可能留下未引用 generation 或发布锁，仍可读取有效 CURRENT，未有有效表时保守回 live。

runtime、CLI 默认输出和 debug 的 manifest 发现共用 `default_baked_out_dir`，所有校验/verify/完整贸易 API 由同一 owner 解析 CURRENT。CURRENT 仅接受一个子目录名，并校验实际目录未越过根目录；没有 CURRENT 时兼容旧平铺 catalog。目录发现先按环境变量 `ARKNIGHTS_INFRA_DATA_DIR`、exe 同目录 `data`、exe 父目录 `data`、当前工作目录 `data` 的顺序选已有 CURRENT/manifest；全部缺失时取第一根。这保留既有默认输出规则：仓库中有旧 `data/baked/manifest.json` 时仍选它；全部删除且无环境变量时 `target/release/infra-cli` 首次生成位置是 `target/release/data/baked`，设置环境变量且无其他已有表时生成到该变量的 `baked` 子目录。环境变量并非对既有 Bake 目录的强制覆盖。CLI 完整生成走同一核心发布 API，limited 生成只允许写独立非 CURRENT 目录，之后原有完整校验仍会拒绝该 limited 表。

黑键—巫恋产品禁配只在读取已验证完整的贸易 Bake 表时过滤，先排除非法行再取前 K 条；它本身不造成 `Required` miss。其他未表示的同站禁配仍由 `Auto` 回 live、`Required` 报错。catalog 的可执行文件指纹失效时，`Auto` 首次尝试重建，`Required` 照常 miss，不能拿旧表证明命中。

核验：`bake validate`、`bake verify`、`infra-cli verify`，并记录输入、组合数、evaluated 和命中模式。候选/剪枝/Bake 改动还要运行最终 assignment invariant gate。
