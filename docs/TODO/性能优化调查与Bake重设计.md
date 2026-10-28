# 性能优化调查与 Bake 重设计

> 文档角色：active-change
> 生命周期状态：proposal
> 当前真源：docs/PERFORMANCE_ENGINEERING.md；docs/QUALITY_AND_AUDIT.md
> 摘要：性能与连续暖机实现、局部数值和最终全表/152盒验收完成；五既有预算失败保留，最终差异归因闭合

当前入口：[最终新语义实测结果](性能优化交接.md#49-最终新语义实测结果2026-10-29)。实现、最终全量/批量验证和覆盖内归因均已完成；下文第一轮未通过与旧语义记录只作历史，不作为重复开工的TODO。

本页是 [性能优化交接](性能优化交接.md) 的展开。交接单记录方向、对拍基线和测量；本页记录**优化前 Bake 阻塞与数据表示调查、当前实现边界和最终验收证据**。性能工程的现行规则在 [PERFORMANCE_ENGINEERING](../PERFORMANCE_ENGINEERING.md)。读完本页不需要再去读 `bake.rs` 全文。

## 0. 当前进度与接手入口（2026-10-29）

> **恢复基线现已完成（23:12）**：独立 release 编译成功，四源码 blob 在采集后仍精确匹配；新二进制 SHA256 为 `6beff764d155986d5daf4989019e211c817d7c87d37a579359bc4de76768340e`，与原 SHA 不同，只声明源码恢复。四轮换共 152 次执行：3、2、12/12/12 各 38/38；24/24 为 33/38，五个失败 box、shift 和涤火杰西卡断言原因与原记录一致。全精二无断言三份产物成功；带断言在 shift2 同一既有失败。证据 `target/codex-runs/perf-final-20261028/baseline-recovered/{report.md,result-summary.json,runs,full-e2-output,full-e2}`。后续比较使用这里的恢复基线；恢复基线不替代最终验收，最终证据见交接单§4.9。

> **2026-10-28 23:00 的历史事故**：本地 WSL 全表生成触发 OOM（内核记录杀死 infra-cli），环境重启后 `/tmp` 冻结基线和临时验收记录丢失；当时 SSH 资源查询被沙箱拒绝。后续已恢复冻结源码与产物、核实实际硬件并按用户授权降低内存继续本地验收，当前状态见下方内存进度；本条不再表示等待执行入口选择。
> 冻结源码已由原会话日志确证：基底 `3201fa7`，仅替换为 `47cbf62` 中四个 U1a 源码 blob；原提交成功 `git apply --cached` 的正是冻结 patch。原基线 SHA256 `315c1954fd3d8c75c10152ec353398734aa0c79ee8397656099833a22b1def07`，与阶段测量 `168a12...` 分开。原二进制及原产物仍丢失，当前精确源码的 release 重建/再采集已完成（见上方最新记录），不把源码恢复写成二进制字节复刻。
> 持久证据改存 `target/codex-runs/perf-final-20261028`：`baseline-recovery/` 保存源码恢复和原会话证据，`baseline-recovered/` 保存重建与采集，`intent-review/report.md` 为独立静态复审（未发现实质问题，不能代替运行验收），`bake-foreground/kernel-oom-session-evidence.json` 保存内核 OOM 原工具输出。此处后续状态优先于本页历史 `/tmp` 路径。

- **U1～U6 已在此前阶段实现并提交；本轮内存及暖机最终实现已完成，任务起点 HEAD 为 e40900f，当前 release 与运行状态见 §0.1。** 格式化/编译证据不能替代全量对账、输出等价或性能收益证据。
- **U1/U2/U3**：U1a 命中字段补全和全量行/查询差分入口（47cbf62）、U1b Auto 首次再生、exe 指纹及失败缓存、generation/CURRENT 原子发布（3ef5f6c）已完成；生成物取消跟踪和 schema4 旧目录清理（dc2af4c）已完成。加载期编号/原子复用（af86268）、贸易/制造借用归并（20fc4d2/e127b28）、房间资源 16 槽数组（458fb33）已完成。完整技能身份（e42f9d0）、真实 pool builder 生成完整变体并全覆盖核对（528b9ea）、schema17 colex rank 直查和 live top-K 比较（2caf24d）已完成。
- **U4 memo 生产接入完成**：Exact 编码（5be9614）、请求 scope/完整叶键/精确 intern/Arc 保活（19c9d1a）、贸易制造搜索接入与命中成本汇总（c29b2b2）、完整排班请求复用（5b165f1）均已提交。命中和减少结算的实际情况由最终日志确认。
- **U5 完成**：制造组合借用和轻量结算（fe53d02）、配方注入缓存（c413336）、贸易借用视图/身份判定/生产搜索贯通（022563c/e421877/9f5fde1）、共用平铺枚举与泛型评估闭包（45dc1fe）已提交。Bake 固定签名槽与生成目录隔离（6a22e21）、miss 分类/冷阶段日志和服务同步预热（2880cc6）也已完成。
- **U6 既有实现（509ad44；后续连续积分修正见§8.1）**：制造暖机按整班时长的逐时平均贡献进入解释器，None 保持 20 小时；心情分段共用整班窗口，未实现非线性积分。含 AddEffRamp 的制造查询池在 Some(h) 下整体回 live，None 仍用固定表；Required 经统一公开门禁严格报 miss，制造 peer-reset 外层绕过路径已删除，CLI 同语义。详见 §4.3。
- **唯一冻结 baseline**：原 `/tmp/perf-current-20261028` 已丢，现使用 `target/codex-runs/perf-final-20261028/baseline-recovered/` 的精确源码恢复与采集产物；四轮换均 38 盒，rot3/rot2/rot12_12_12 成功，rot24_24 保留 5 个已知断言失败。全精二 `full-e2-output` 无断言产物成功，`full-e2` 带断言保留同类失败。详见交接单 §4.1；不在 U6 前另建基线，旧 e976b1a 仅历史参考。
- **当前实现与最终运行已完成**：旧语义消融覆盖内纯性能归因关闭；新连续暖机局部数值/P1P2静态闭合、最终完整Bake及152盒/full-e2统一包完成。但书PASS，原五例24/24预算失败逐字保留，无新增/移除；不扩大排班policy、不把正常warning fallback写成预算通过。最终5949叶差异归因已闭合，文档/索引按交付收口，不重新生成Bake或重复旧实现TODO。
- 基础汇总日志（4d057a3）和批次/阶段归因（65355b2）已完成；冻结版本单 box 三次测量只作历史归因证据。用户不要求实际性能指标，日志可随最终运行留存，不额外开启性能测量任务。
- 删除源码/Python 测试及无引用 fixture、CI 和文档清理（0d8b935）、泛型轮换入口及相关文案已完成。CRLF/LF、default_dorm_beds 和两条可见性 warning 不处理；box-bundles 已复制但不入库。1+1 和四赤金本轮不开工；共享工作区单 writer，git 操作由主代理负责。

### 0.1 本轮内存降低与最终验收（2026-10-29）

> 第一轮历史运行事实：`memory-reduction/bake-all-final` 已真实结束。schema17 共 29,783,417 行、13 个非空 signature、674 个完整状态，文件 3,843,098,117 字节；`rows_checked=29783417 row_mismatches=0 queries_checked=3292 query_mismatches=0 skipped_signatures=0 skipped_queries=2148`，CURRENT/磁盘 header/manifest/summary 一致。systemd 记录内存峰值 9.2G、swap 0B。该次 CLI exit 1 唯一失败为 `reg_gsl_docus_solo` 的过期期望1.907，实际1.909与固定单位产出锚和千分结算一致；当轮CLI仍为历史exit1；后续最终7695096fd回归exit0/但书PASS，见§4.9。第一轮四轮换对拍也已结束，历史输出差异与失败集合见下方，当前正确性结论见§8.1。

- **已完成并验证**：signature 独立完整排序/落盘、组合迭代 `par_bridge`、schema17 流式拼接与原 bytes hash 合同、typed compact 常驻行、同一 File 流式冷校验、rank bitset 完整 universe 核对、4096 行全字段 verify 与实际 compact 命中对账入口已实现。数值保持原 `i32`/`f64`，热查询保留 rank 数组直接查找及原比较规则，完整贸易 API 保留 signature+offset 并按需 owned 输出；320 查询覆盖不减少。格式化、release 编译与两项静态契约审查已完成，任务起点HEAD为`e40900f`，当前最终结果见交接单§4.9。
- **运行反证与纠正**：此前静态把“尝试六个贸易 signature”误认为“保存六个非空 signature”，曾扩成 15 槽。真实全量生成中 lv1/lv2 源石均为零行，原四个贸易+九个制造/13 槽正确；已恢复原 expected 集合与槽位，保留全部内存改造。前轮主动 TERM 停止，`systemd-run` 外层返回 0 不表示 Bake 成功，主进程是 `code=killed/status=TERM`，未完成生成或验证；证据 `memory-reduction/bake-all/observed-stop.json` 保留，不复用其表作为最终有效表。
- **第一轮历史运行已结束**：当轮 release SHA256 为 `8f9b7a40c2eade5e3a5c972cf7396c3ff20894d0c724f54cc14da549bef1c8e5`，完整 Bake 和最终包起止哈希一致。Bake 在本地 WSL 以 `MemoryMax=14G`、`MemorySwapMax=2G`、`RAYON_NUM_THREADS=8` 完成；原命令/日志在 `memory-reduction/bake-all-final/`，隔离表为 `memory-reduction/baked-final/`，未写 `data/baked/`。机器摘要 `memory-reduction/bake-validation.json`；原 `next-validation.json` 是执行前计划，旧服务器包仅阶段历史。核心对账通过不等于 CLI 回归或最终输出等价通过。
- **已授权的环境处理**：Windows 32GiB，本地 WSL 16GiB+4GiB swap；服务器 `114.66.55.78` 已连通，但只有 16GiB、约 11GiB 可用且无 swap，不能作为更大内存入口。用户已授权内存降低和无关进程清理无人值守推进；QQ/Steam/Everything 用户进程已停，系统、代理、远程连接、当前 Codex 与 WPS 项目文档保留。执行入口选择已结束，不再等待用户选择服务器或本地分块。
- **第一轮历史包实证（当时未通过）**：四轮换各 38 盒，共 152 次、147 完成、5 断言失败；`jobs1` 仅降并发，覆盖与冻结基线完全相同。3、2、12/12/12 各 `same=0 changed=38`，24/24 为 `same=4 changed=34`，四命令均 exit1。前三轮无断言失败；24/24 移除 b-101-424 的既有失败、新增 b-112-404 同类 shift2 涤火杰西卡失败，另四条完全一致。全精二无断言三产物均变化；带断言旧 exit1、新 exit0，不能算旧新状态相同。原始差异、全部子命令元数据与失败集合在 `memory-reduction/final-batch/`；不得剔除新增差异或更换基线。第一轮按当时仅允许暖机房变化的限制未通过；最新用户已要求按正确机制处理暖机，不以旧名单不变限制正确重选，仍须核对连带变化来源。五例失败断言已证实无误报；正常warning fallback不代表预算合格。未提交源码、未宣称性能收益。

## 1. 用户裁决（2026-10-28，原意）

- 目标：优化到非常快，少写手工规则，可以做不安全的剪枝、较大的枚举，并用 Bake 省重复任务；“不需要在意本项目的算法设计，一切只为了性能和结果”。
- 只做最终一次对拍，开发中不逐步检查。结果仍须正确。
- Bake 可以重新设计。**心情除铅踝外不影响真正效率**；暖机角色的效率可以额外写函数决定；即使 Bake 不能覆盖铅踝和暖机技能，也只需要为暖机角色做热路径计算。
- v3 的预编译（`build.rs` 编号化）和定长表要移植到 v2；v3 的单房结算写法与 v2 不同，可以节省时间。
- 当前裁决足以支持改完 Bake 和定长数组的效率计算。进入无人值守：不需要用户裁决的部分尽量直接改，需要裁决的统一写文档（§8）。
- 用户认为 Bake 修复和修改干员技能表是低危。**纠正**：语义风险低（前提是输出逐字节不变），但改动面是全部解释器与全部房间搜索，必须靠 §6 的对拍验收，不能凭“低危”省掉。

用户原话（2026-10-28，本轮会话）：

> 本项目还是为了最高的单日产出和最快的速度。

> 不需要每一步都验证，跑现在这个版本和最终版本的对拍就可以

后一句覆盖本页旧的“每单元四轮对拍”条款：冻结时含未提交 U1a 的版本作为 baseline，最终一次统一验证；中途不运行源码测试、不逐单元验收，必要编译允许。

本轮补充裁决（2026-10-28，会话裁决摘要，非逐字引用）：

- 删除 GPT 写的全部源码测试，含 Python 测试和无引用 fixture；CI 改用 cargo check，文档清掉 cargo test 入口。已提交 0d8b935，不重开清理或新增源码测试。
- 泛型两班/三班轮换的 CLI 只认 `3`、`2`、`12/12/12`、`24/24`、`main-backup@H1/H2`、`abc@H1/H2/H3`；`abc_12_6_6` 是 wire/profile 值，不是 CLI 参数。相关文档和错误提示已更正。
- CRLF/LF、default_dorm_beds 和两条可见性 warning 不处理；box-bundles 复制用于验收，不入库。局部顺手小修自主处理，不反复询问。
- 调查已够用；确需大调查时，每个 subagent 只回答一个具体事实，最多 15 次工具调用，边查边写临时报告；主代理复核影响裁决的证据及关键否定结论，不忽略已有回答。
- 做完写入 docs 并给可复制接手提示；需要裁决时提供二选一，推荐项在前。协作 memory 已有，不重复记录。

## 2. 对拍与测量入口

详见交接单 §4.1。要点：

- 本轮 baseline：`target/codex-runs/perf-final-20261028/baseline-recovered/` 的精确冻结源码重建与采集产物（原冻结含未提交U1a）；四轮换及全精二产物/断言状态见交接单§4.1，不用后续HEAD代替。旧 `/tmp/perf-baseline/infra-cli-baseline`（e976b1a）仅保留历史参考，不用于本轮最终验收。`/tmp` 重启会丢，需保留冻结版本与构建/输入指纹才能重建。
- box 集合 `data/fixtures/box-bundles/`（`.gitignore` 忽略，已复制进 main 工作树；38 个）。
- 最终对拍命令形态：`python3 scripts/assert_rotation_invariants.py --group all --rotation 3 --binary target/release/infra-cli --skip-build --jobs 1 --output-dir target/codex-runs/perf-final-20261028/memory-reduction/final-runs/rot3 --compare-to target/codex-runs/perf-final-20261028/baseline-recovered/runs/rot3`；`--rotation` 依次用 `3`、`2`、`12/12/12`、`24/24`，对应 `baseline-recovered/runs/` 已采集的四种轮换目录；在同一仓库 cwd 执行，降低并发不减覆盖。只在当前版本与最终版本各跑一轮完整包。
- 旧 e976b1a 基线里 `24/24` 有 5 个 box 因涤火杰西卡的 `main_backup_24_24_work_budget` 断言失败。这是历史事实；本轮以当前冻结 baseline 的实际失败集合和原因作比较，超范围差异原样报告，不放宽验收。
- 最终包继续覆盖38盒及 `data/fixtures/243/operbox_full_e2.json` 的 `24/24`；全精二无断言产物与带断言失败分开比较。U3已支持合法E0/E1状态，不再从“存在非精二干员”推断普通box必miss，实际命中以最终日志确认。
- 单 box 耗时：`target/release/infra-cli profile layout-full --operbox <box> --runs 1`。

## 3. 优化前基线：已核实的事实

本章记录优化启动时的调查，schema15、固定精二池、指纹重复哈希、全signature扫描和命中字段缺失均指旧基线，旧行号与原始测量保留作证据，不能当作当前实现。当前schema17/完整变体/rank查询见§0、§4及 [PERFORMANCE_ENGINEERING](../PERFORMANCE_ENGINEERING.md)。主代理回源码或原始输出核对过的标 ✔。

### 3.1 耗时分布

- ✔ b-008-411，三班，`profile layout-full`：总计约 2.0s；`schedule_team_rotation` 1668.5ms（82.7%），`assign_base_greedy` 336.5ms（16.7%），单次贸易和制造搜索各 5～6ms。
- ✔ `trade_solves=15379`、`exclusive_checks=18815`，而该 box 的单班贸易池只有 340 个组合：排班阶段把同一批组合重复结算约 45 倍。只测了一个 box。
- 重复结算的调用点分析见 §3.7。

### 3.2 Bake 的结构

- ✔ 只覆盖贸易与制造。key 是字符串 `trade:level{L}:cap{C}:order_{Gold|Originium}[:gold_lines{n}]` 与 `manufacture:level{L}:cap{C}:recipe_{...}`（`bake.rs:2197-2222`）；共 13 个 signature；`combo_table.bin` 134315 行约 20MB，bincode 默认配置，无魔数。
- ✔ 生成：以固定假设跑 live solver 的全组合枚举：池 75 个干员（制造按配方过滤为 22/15/15），全部精二 1 级 6 星，心情 24，`LayoutContext::search_baseline()`（`bake.rs:664-1146`）。全量生成 772ms（`summary.json`）。
- ✔ `BAKE_SCHEMA_VERSION = 15`（`bake.rs:39`）。`data/baked` 进 git（12 个文件），其中 `aic-baked-save/` 下是 schema 4 的旧 JSON 版，与新版共存。

### 3.3 为什么 Bake 从不命中（四层阻塞，任何一层都足以 miss）

1. ✔ **指纹**：`validate_baked_catalog` 要求 manifest 里的生成器指纹（`infra-cli` 可执行文件的字节数与 hash64，hash 是对整个 exe 文件字节做 `DefaultHasher`，`bake.rs:2224-2235`）与当前运行的 exe 完全一致（`bake.rs:1241-1252`）。任何重新编译、任何别的机器都失效；仓库提交的 manifest 指向另一路径下的旧二进制。
2. ✔ **校验失败被静默吞掉**：`load_runtime_baked_table`（`bake.rs:2133-2149`）`Err(_) => Ok(None)`，`BakeMode::Auto` 下回退 live，没有任何提示；只有成功才写入 `OnceLock` 缓存，失败后**每次调用重新读 15MB exe 并重新哈希**。
3. ✔ **查询条件**：
   - 贸易 `baked_trade_compatible`（`bake.rs:2237`）：`mood == 24`、`shift_hours == 24`、layout 等于烘焙基线、不需要候选投影、池内**所有**干员 `elite >= 2`、无 `must_operator_override`。
   - 制造 `baked_manufacture_compatible`（`bake.rs:2270`）：`!full_pool`、`mood == 24`、`shift_hours == None`、layout 兼容、池内所有干员 `elite >= 2`。
   - 排班路径：制造 fill 固定 `full_pool: true` 且 `shift_hours: Some(..)`（`manufacture_fill.rs:636-645`），必 miss；贸易 fill 走 `Auto`、`full_pool=false`，但轮换里班次时长不是 24 小时就 miss。
4. ✔ **精二条件对真实 box 恒不成立**：对 38 个样本 box，烘焙池 110 个干员里拥有的平均 85.7 个，其中精二以下的平均 63.4 个（最少 16 个）；38/38 个 box 至少有一个，所以 `all(elite >= 2)` 对每个真实 box 都为假。烘焙池把所有人固定为精二，而技能解锁取决于精英阶段（tier_0 与 tier_up），不是简单的“精二更强”。

另有：规则路径（`rules.rs:869/1550/1700` 与 `manufacture_fill.rs:297` 的锚定搜索）硬编码 `BakeMode::Disabled`，不查表。`BakeMode::Required` 全仓库没有构造点；CLI 没有任何参数能设置 BakeMode，所有路径靠 `Default`（`Auto`）。**交接单 §2 “规则路径 bake_mode 为 Disabled” 只对规则路径成立，fill 路径本来是 `Auto`。**

### 3.4 v2 的数据表示与单组合开销

- ✔ `SkillTable { by_id: HashMap<String, SkillDef>, skills: Vec<SkillDef> }`（`skill_table.rs:18`），`data/skill_table.json` 326 条、168619 字节；`crates/infra-core` 无 `build.rs`；依赖 serde、serde_json、bincode、thiserror、csv、rayon、calamine。
- ✔ 解释器按 `buff_id`（`String`）查表，不按干员名（`trade/interpreter.rs:203-205`）。
- ✔ 每评估一个贸易组合：`TradeContext::from_room`（`trade/interpreter.rs:55-75`）克隆每个干员的 name/`buff_ids`/`tags`、克隆整个 `layout`；`current_room_tag_counts`（`trade/solver.rs:258`）每次新建 `HashMap<String, u8>`；`apply_trade_phases` 在非预编译路径里还 `collect` 一份名字 `Vec<String>`。
- 单组合评估入口是 `trade/solver.rs:148 solve_trade_with_shift_prevalidated`（不在 `search/trade.rs`），由 `search/trade.rs:551 eval_combo_hit` 调用。
- 搜索：`search/trade.rs:376 combinations_indices` 全量 C(n,k)，逐组合完整结算，保存全部命中后排序取 best 与 top_k。

### 3.5 预编译现状（`compiled_atoms`）

- 贸易侧**已有一半预编译**：`CompiledAtom { atom: EffectAtom, sort_key: (i32,i32), seq: u16 }`（`types.rs:436`），建池时每个池条目编译一次（`pool/trade.rs:17 compile_operator_atoms`、调用点 `:340`），承载为 `Arc<[CompiledAtom]>`；每组合只做 Arc 克隆。解释器在所有干员都有预编译原子时走 `collect_atoms_merged` + `apply_atoms_loop_compiled`（`trade/interpreter.rs:220-273`），否则退回旧路径。
- 制造侧**没有**预编译路径：`collect_manu_atoms`（`manufacture/interpreter.rs:197`）每次展开 `skill.atoms` 并排序。
- 内层 `EffectAtom` 仍含 `Option<String>`（`atom_id`、`tag`）和带 `String` 的 `Selector`，不是纯 POD。
- 反例：`schedule/team_rotation.rs:5358` 手工构造 `compiled_atoms: Arc::from([])`，走旧路径。

### 3.6 心情与暖机（`skill_table.json` 全表扫描，原文 `/tmp/v2-mood-ramp/report.md`）

- 贸易站：没有任何效率随心情变化的原子；19 个心情原子全是 `mood_drain_delta`/`mood_drain_per_state_step`。
- 制造站：心情改变效率的只有铅踝两条：模糊视线 `manu_prod_spd_reduce[000]`（`owner_mood_gap` selector，每 4 点落差 -5%）和窗外雪啸 `manu_prod_spd_addition&cost[000]`（落差 >12 时 +10% 效率、+6 仓库）。全表没有其他技能使用 `owner_mood_gap`。
- 控制中枢 3 个心情条件技能（山河远阔、不以物喜、不以己悲）不在单房间 Bake 范围内。
- 暖机（`add_eff_ramp`）共 5 条，全在制造站：急性子 `[030]`、“等不及” `[031]`、慢性子 `[040]`、延时摄影 `[041]`、例行清扫 `[100]`；发电站另有两条硬编码 ramp（`power/solver.rs:112/120`，技术交流 α/β）。
- ✔ **制造站的 ramp 与 `shift_hours` 无关**：解释器固定取 20 小时算术平均（`manufacture/interpreter.rs:570 eff_ramp_avg_20h`；`eff_ramp_at_shift_hours` 只有发电站调用）。`solve_manufacture_with_shift`（`manufacture/solver.rs:59-173`）仅在房间含心情落差效果（即铅踝）时按心情阈值切段重解，否则退回静态结算。所以**暖机技能的现行效率是常数，可以直接烘进表**；“暖机效率随班时长变化”是行为变化，见 §8-3。
- ✔ 两处 `mood <= 0` 把基础效率归零（`trade/interpreter.rs:140`、`manufacture/interpreter.rs:135`），Bake 必须在心情 ≤ 0 时回退 live。
- 字段口径澄清：`trade/unit_output.rs` 的 `shift_hours/24` 缩放用于 `TradeResult.daily_at_shift`，不改变纸面效率；搜索命中里的日产字段仍按24小时恢复，Bake命中不得再次乘该比例。早期“查表后缩放产出”的泛化描述作废。

### 3.7 Bake 查表算法与命中结果（原文 `/tmp/g1/report.md`；✔ 为已回源码核对）

- ✔ 查表步骤（贸易 `bake.rs:1953-2049`，制造 `2051-2121`）：`Disabled` 直接 miss → 非全局 `forbidden_pairs` miss → 兼容性不满足 miss → 加载表并检查池内干员名全在烘焙池 → 拼键取 `(start, len)` → **线性扫描该 signature 的所有行**，每行 `mask_subset(row.operator_mask, available_mask)`（按 u64 字块 `word & !available == 0`，`bake.rs:2352`）→ 必含名、禁配对逐行 `contains` → `row_to_trade_hit` → `hit_filter` → 凑满 `top_k` 就 `break`。没有按 bit 建的倒排索引。
- ✔ 并列顺序**不在运行时比较**，由烘焙期 `compare_baked_rows`（`bake.rs:1190`）预先排好：`sort_efficiency` 降序 → `manufacture_storage_limit` 降序 → `operator_indices` 升序；运行时第一条命中的行就是 best。`evaluated` 是命中数，不是扫描行数。
- 贸易 lvl3 signature 有 64294 行；正常情况前几行就凑满 `top_k`（默认 5），无命中时才全扫。
- ✔ **候选投影**：`trade_pool_requires_candidate_projection`（`pool/trade.rs:208`）——池里任一干员带 `TAG_DURIN`、金流技能，或预编译原子的条件是 `OperatorInBase`/`OperatorInTrade`、selector 是 `TaggedCountInTradeSum`（`pool/trade.rs:184-192`），整个池就不能用 Bake。这类干员的效率取决于房间外或同房的其他人，不是房间内自足的，键里需要把这部分上下文放进去。
- ✔ **命中结果不完整**：`TradeSearchHit`（构造于 `bake.rs:2370-2411`）的 `gold_names`/`originium_names` 为空，`breakdown` 用 `Default` 兜底；`ManuSearchHit`（`bake.rs:2441-2465`）的 `gold_names`/`battle_record_names` 为空，`unit_output_per_day`/`output_per_day` 为 0，只填当前配方对应的 per_station/storage 字段；两者的订单线/生产线字段（`gold_order_line` 等）固定 `None`；任一必需 `Option` 缺失则整行被跳过。下游读取情况（原文 `/tmp/g5/report.md`）：
  - ✔ `ManuSearchHit.breakdown.unit_output_per_day` / `output_per_day` 被 `layout/assign/commit.rs:121-134`（`manu_efficiency_snapshot`）原样写进 `RoomEfficiencySnapshot`，又被 `box_profile/eval.rs`、`schedule/base_rotation.rs:172-173`、`team_rotation.rs:494-504` 读取计产出。Bake 命中的这两项是 0，**命中后产出统计会错**。
  - `gold_names`/`originium_names`/`battle_record_names` 被 `commit.rs:89-93`、`candidate.rs:133/189`、`infra-cli/src/output.rs` 读取；`gold_order_line`/`originium_order_line`/`gold_line`/`battle_record_line` 被 `box_profile/build.rs`、`output.rs` 读取（`None` 时整块跳过）。
  - live 单订单搜索本身也把 `gold_names`/`originium_names` 设为空 `vec![]`（`search/trade.rs:584-585`），所以这两项只在 live 双订单搜索里有值。
  - 结论：**现有 Bake 命中结果不能当作与 live 逐字段一致**。重设计必须在命中后补全这些字段（用函数从行数据重算，或在行里存全），然后才能声明 safe reduction。
- ✔ **并列排序与 live 不一致**：贸易 live 只按 `final_efficiency` 单键降序（`search/trade.rs:429`，稳定排序，同分保持枚举顺序）；制造 live 是效率降序 → 仓库容量降序 → **干员名字典序升序**（`search/manufacture.rs:378-389`）；Bake 是 `sort_efficiency` 降序 → 仓库容量降序 → **烘焙池下标升序**（`bake.rs:1190`）。末级键不同，同分组合的 best 可能不同。重设计要按 live 的实际比较规则排序，并在 §6 的全量对账里包含 best 与 top_k 顺序的比较。

### 3.7b 排班里重复结算的来源（原文 `/tmp/g3/report.md`；✔ 为已回源码核对）

默认三班（`Abc12_6_6`）的调用结构，行号在 `schedule/team_rotation.rs`，除非另注：

- `schedule_team_rotation`（`:4683`）→ `schedule_timed_rotation_inner`（`:7270`）→ `schedule_abc_rotation_candidates`（`:6760`）。
- **N / 菲亚 A / 菲亚 B 三个候选各自完整求解一遍**，再比较（`:6820`、`:6913`、`:6984`）。
- ✔ 每个候选经 `finish_abc_candidate_with_control_rebuilds`（`:6719`）：`for control_attempt in 0..=3`，中枢心情被拒时**整个候选重建**，每候选最多 4 次。
- 每次重建（`schedule_abc_rotation_once`，`:7700`）内：peak 编制与分配一次（`assign_shift_with_plan_skip`，`:7766/:7780`，内部 `build_plan` 一次）、γ 两个半天各一次（`assign_gamma_half`，`:7992/:8036`）、源石 pair peak 一次（`:8959`）、每个深海 S2 替补候选一次（`:8619`）、每个菲亚/巫恋替补候选一次（`apply_profile_fiammetta_rotation` `:8847`，内部再调 `refill_trade_room_with_core`/`refill_manu_room_with_core`）。
- 每次实际搜索的入口链：`search_trade_triples_filtered`（`search/trade.rs:178`，内部对三种订单各一次 `search_trade_single_order`）← `pick_trade_hit` ← `assign_trade_remainder_with_plan_anchors`（`trade_fill.rs:124` 的 `while` 逐个贸易站）。制造同理（`manufacture_fill.rs`、`team_fill.rs:267` 逐房间、`rules.rs:1553/1702` 逐必含干员与逐房间×配方）。
- ✔ **没有任何跨调用的搜索结果缓存**。`grep cache|memo` 的命中只有 `BaseSystemsCache`（系统列表）和 `trade_segment_cache`（贸易 segment 表），都不是搜索结果；`trade_fill.rs:738` 的 `base_sub`/`full_pool` 只是每次调用内克隆池子。
- 乘法结构：候选数（3）×中枢重建（最多 4）×每次重建内的 peak/γ×2/源石/替补候选 × 每个房间的三种订单搜索。实测 340 个组合对应 15379 次结算，与这个结构吻合，但**具体哪一层贡献最大还没有分解**（需要在 `record_trade_solve` 处按调用阶段打标签，见 §7）。

对设计的含义：Bake 只能加速“单次搜索内的枚举”；排班里更大的乘数是“同一个（池、订单、等级、gold_lines、上下文）被重复搜索”，需要 U4 的记忆化，这与 Bake 是互补的两件事。

### 3.8 运行期数据覆盖点与干员规模（✔ 主代理直接核对）

- ✔ 数据文件在运行期**可以被替换**。`data_path(name)`（`skill_table.rs:141`）的解析顺序：环境变量 `ARKNIGHTS_INFRA_DATA_DIR` 指定的目录（文件不存在则报错）→ exe 旁的 `data/`、exe 上一级的 `data/`、当前工作目录的 `data/` → 编译进二进制的内嵌副本（`include_bytes!`，`skill_table.rs:256-291`）→ 工作区根的 `data/`。CLI 没有“指定技能表路径”的参数，所有入口都调 `default_skill_table_path()` 再 `SkillTable::load`（`main.rs`、`commands/layout.rs`、`commands/plan.rs` 等）。
- 含义：`build.rs` 编译期固化 `skill_table.json` 会让 `ARKNIGHTS_INFRA_DATA_DIR` 与 exe 旁 `data/` 的覆盖失效，这是行为变化；除非先确认没有人依赖运行期换数据（Worker 部署是否带外部 `data/`，本页未核实，见 §7）。加载期一次性建稠密表（§8-2 的选项 B）不受影响。
- ✔ 规模：`data/operator_instances.json` 的 `instances` 有 573 条，键形如 `CONFESS-47@tier_0`（干员名@档位），每条含 `tags` 与按设施分的 `buff_ids`；引用的不同 `buff_id` 共 451 个；`skill_table.json` 有 326 条技能，其中 126 个被引用的 `buff_id` 不在技能表里（解释器对查不到的 `buff_id` 直接 `continue`，`trade/interpreter.rs:203-206`），技能表里只有 1 条未被引用。即稠密编号要覆盖 451 个 `buff_id`，其中 126 个没有原子。
- 干员的实例键是“名字@档位”，同一干员的 tier_0 与 tier_up 是两个实例，这正好是 U3 里“每个干员的技能状态变体”的现成载体，不需要另造。
- 烘焙池只有 110 个干员；v3 的全量是 429（`OPERATOR_COUNT`），v2 的 `instances` 去重后的干员数量未单独统计。

### 3.9 v3 的做法（`/home/knightcode/Projects/ArknightsInfraCalc-v3`，只作性能参考）

- ✔ `build.rs`（3175 行）读 10 个输入（`data/*.csv`、`operator-catalog.json`、`building-skill-catalog.json`、`term-catalog.json`、`data/candidate_catalog.json`、`data/effect_registry.json`、`data/mood_skills.csv` 等），生成 `OUT_DIR/generated.rs`：`pub type OperatorId = u16`（`src/facility_state.rs:13`）、`OPERATOR_COUNT = 429`、`SKILL_FUNCTIONS: [SkillFn; 705]`、`OPERATOR_SKILL_VARIANTS: [[[Oper; 2]; 4]; 429]`（干员 × 4 技能槽 × 2 变体）、干员标签位图、`MOOD_RULES: [MoodRule; 160]`。
- ✔ 技能结算是函数指针：`SkillFn = fn(&mut Runtime, u16, i32, i32, i32, u8)`（`src/lib.rs:18`），指针存在 `Oper` 结构里，调用点 `src/lib.rs:93`。**v2 是 `EffectAtom` 数据驱动的解释器，不是每技能一个函数；这个差别不能抹掉，见 §8-1。**
- ✔ 房间状态是定长数组加计数：`RoomAssignment { operators: [OperatorId; N], count: u8 }`，`MAX_ROOM_MEMBERS = 5`；临时空间 `RoomScratch` 是 `[i32; OPERATOR_COUNT]`；超过 5 人退回 Vec 路径。
- ✔ 单房搜索 `search_room_combo(capacity, candidates, required, evaluate)` 的结算函数由闭包传入，搜索与结算解耦；房间结算签名 `settle_trade_combo(level, order, &[TradeMember]) -> TradeSettlement` 不收也不返回堆容器。
- ✔ **v3 没有烘焙表**：源码里没有 `combo_table`、`include_bytes`、`bincode`；`bake_teammate_pairs` 只是现算的 Vec，注释自述是“后续把运行时 C(n,k) 替换成烘焙表查询的第一步数据结构”。v3 的速度完全来自编号化、定长数组和函数指针。Bake 没有 v3 可抄，只能在 v2 里重新设计。
- v3 的 AGENTS.md 有两条可采用的约束：热路径查询只读定长数组，不引入哈希集合、字符串扫描或查询时遍历成员；机制分支走编译期整数编号或数据表。v3 的 AGENTS.md 也禁止把 Bake 当机制语义，与用户现在的方向相反，**不带入 v2**。
- v3 的数据源是它自己的 CSV/JSON，格式与 v2 的 `skill_table.json` 不同，`build.rs` 不能直接复制。

## 4. 设计

### 4.1 要解决的问题与唯一责任边界

不变量：**同一个房间输入在同一份数据下，快路径（查表或定长评估）的结算结果必须与 live 解释器逐字段一致。**

当前责任边界：

- Bake存储域使用完整技能身份 `(name, 原始elite, ordered buff_ids, tags)`；等级/稀有度通过真实pool builder的五类代表练度影响实际解锁，按完整身份去重，组合禁止同名。schema17每signature在冷加载建立dense colex rank索引；热查询沿live子池相对顺序枚举，rank直查和轻量top-K比较保留live顺序，缺状态或非canonical顺序回live。
- 查询上下文须通过完整 `LayoutContext::search_baseline()` 门禁（含active_source_buffs、浮点有限性与bit、注入顺序与聚合结果），不是任意layout都编码进Bake表。未覆盖动态贸易注入、制造count-scaled注入、Mood/OwnerMoodGap相关技能或制造 peer-reset 继续 live，并经统一公开门禁产生 typed miss；Required 严格报错。心情须有限且>0，班时长须有限；full_pool与E0/E1本身不再导致miss。
- 指纹保留可执行文件与输入数据，成功/失败在进程内缓存；Auto首次缺表/失效以Disabled live生成，全量核心校验通过才原子发布，Required严格只读。`data/baked/`已忽略并取消跟踪，schema4旧目录已删除。
- 解释器保留 v2 的 EffectAtom 语义；U2/U5 复用编号、借用组合数据、轻量结算与平铺枚举。U4 Exact 编码和完整叶键已接贸易/制造及完整排班请求 scope，memo已在生产调用中生效，最终统一包和旧语义消融已完成覆盖内输出对照；不外推任意输入或实际性能收益。
- 贸易hit的日产字段用单位值与total_efficiency按24小时恢复，**不得按shift_hours/24再缩放**；只有`TradeResult.daily_at_shift`表示实际班次量并缩放。制造暖机 Some(h) 使用真实连续时间积分，None 默认 20 小时；用户授权正确暖机以及可证实的连带重选，见 §4.3。

Bake层拥有存储完整性、上下文准入与live回退；搜索层拥有枚举、合法过滤、比较与输出合同。旧语义全量对账及消融已完成，最终连续暖机新表verify及CLI回归已完成；验证只约束实际表/查询/输入覆盖，不提前声明任意查询普遍等价。

### 4.2 保证等级声明

- Bake 键完整且表由同一个 live solver 生成，查表结果与 live 逐字段一致：**safe reduction**。声称成立的前提是 §6 的全量对账通过。
- 未覆盖的上下文通过Bake门禁回live；有限正心情与班时长按现行字段口径处理；Some(h) 且池含暖机时整体回 live，None 保持固定表。门禁与现场结算不改变候选集合，不是剪枝。
- 任何“不安全剪枝、更大枚举”（用户愿意接受的方向）：这一轮**不做**；一旦做，须标 heuristic 或 approximation，并按质量文档处理，不得声称全局最优。

### 4.3 工作单元（实现结果）

各单元可独立提交，由主代理负责 git 写操作；中途不逐单元对拍或验收，必要编译允许。代码已完成，最终统一包按 §6 验收。U1～U5 必须保持结果，U6 仅允许授权暖机行为差异。

- **U1a 已实现：命中补全与全量对账入口**：持久化/恢复完整命中数值，行verify重算完整响应并做确定性查询差分；制造产出字段和live排序口径已补齐。最终新语义schema17完整表29,783,417行与3,292兼容查询对账零差异，跳过2,148查询；最终CLI exit0，但书期望修正后回归PASS。
- **U1b 已实现：运行时再生与发布**：Auto缺表/失效一次Disabled live生成，exe指纹及失败缓存，验证后独立generation与CURRENT原子发布；Required严格只读，CLI/debug共用解析。目录清理已完成；固定 BakeMissKind 分类、query_errors 与冷阶段 calls/errors/nanos 汇总及 serve 同步预热均已提交（2880cc6），Required 严格只读。
- **U2 已实现：加载期编号、借用与定长资源槽**：干员/buff稠密编号，建池预编译复用，贸易/制造借用输入与稳定归并，贸易/制造房间资源16槽数组。GlobalResourcePool与中枢/办公室/发电快照保留现有owner边界；不扩展候选准入与规则语义。
- **U3 已实现：完整变体存储与rank直查**：身份为name/原始elite/ordered buff_ids/tags；全部实例设施绑定经真实pool builder生成E0/E1 tier_0、低星等级解锁、E1/E2 tier_up五类代表练度，按完整身份去重，制造各配方使用同一完整变体池，同名组合互斥。完整legal combo集合冷校验，每signature建立checked dense colex rank数组；热查询沿live原枚举次序直查，有界堆按贸易效率/枚举序与制造效率/仓库/组合姓名比较，只物化best/top，evaluated保留全部合法过滤组合。完整layoutbaseline及技能原文/compiled atom身份门禁仍在，未覆盖动态或心情敏感条件回live。贸易 hit 日产不按 shift 再缩放；制造表生成和行 verify 均以 None 的 20 小时暖机值重算，Some(h) 且池含 AddEffRamp 时整体回 live。Bake 物理 signature 使用固定槽，不在热路径拼字符串；可用 `ARKNIGHTS_INFRA_BAKED_DIR` 显式隔离生成目录（6a22e21），默认目录发现顺序保持原规则。完整表和加载期索引已裁决，不因体积/冷启动重新请求选择。
- **U4a/U4b 已实现并接生产**：Exact 编码（5be9614）、TLS 请求 scope/完整叶键/精确字节 intern/Arc 保活（19c9d1a）、贸易制造叶搜索及成本汇总（c29b2b2）、完整排班请求复用（5b165f1）已提交。键覆盖、嵌套生命周期、返回顺序和输出等价由最终包验证；不把“trade_solves 接近组合数”写成保证。
- **U5 已实现：搜索与结算解耦**：共用平铺组合枚举接受泛型评估闭包（45dc1fe）；制造借用池数据与轻量结算（fe53d02）和配方注入缓存（c413336）、贸易房间借用视图及 shortcut/链段/搜索贯通（022563c/e421877/9f5fde1）均已提交。枚举顺序、合法过滤与 live 比较口径保留；本单元未增加不安全剪枝。
- **U6 既有实现：暖机随整班时长变化（509ad44；最新线性修正已完成，旧实现记录保留；当前见§8.1）**：有限的 `Some(h)` 先 `round` 再 `clamp(1, 24)`，暖机贡献取该窗口内逐时加成的算术平均，然后进入现有阶段解释器；`None` 保持 20 小时。非有限时长拒绝；`layout/assign` 默认 24 小时也会使用 24 小时平均。心情分段重解共用整班暖机窗口，未做逐时非线性积分，不能据此声称精确积分语义。
- **U6 Bake 门禁**：`Some(h)` 且整个制造候选池含任一 `AddEffRamp` 时，查询整体回 live；`None` 固定表仍按 20 小时生成/verify。公开 `try_baked_*` 统一判定；Required 对暖机、peer-reset 等 miss 严格报错，制造 peer-reset 外层绕过路径已删除；CLI 使用同一语义。
- **最终差异边界**：最终版本统一与 `target/codex-runs/perf-final-20261028/baseline-recovered/` 比较，不在 U6 前另建 baseline。第一轮“仅含五暖机技能的房间可变化”限制及实际超范围差异保留作历史；用户最新要求按正确暖机机制实现，不以旧名单限制正确重选。直接效率变化和连带选择仍分别核因，纯性能等价、实际超预算与未归因偏差不因此豁免，详见§8.1。

### 4.4 移植 v3 时的取舍

- 移植的是**机制**：编号化、定长数组、临时空间复用、搜索与结算解耦。不移植 v3 的规则行为，也不移植它的函数指针式技能模型。
- 技能结算仍沿用 v2 的 `EffectAtom` 解释器，避免丢失 v2 保留的规则语义（用户 2026-10-28：v3 丢了很多规则设计，v2 是语义参考和对拍基准）。

## 5. 不做什么

- 不为单一 box、房间号、班次下标写补偿特判。
- 不放宽对拍：任何产物差异（除用户裁决过的）都视为未完成。
- 不碰 Mower 动态换班、四赤金、1+1 自动轮换（交接单已排在性能之后）。
- 不把 v3 AGENTS.md 里“禁止 Bake”的规则带进 v2。

## 6. 验收与基础性能日志

- **当前版本与最终版本一次统一对拍**：使用 `target/codex-runs/perf-final-20261028/baseline-recovered/` 的精确冻结源码恢复及产物作为 baseline；U1a～U6 已实现，最终统一跑 §2 四种轮换全量对拍、全精二产物与已知断言失败样本、Bake 全量对账、终态断言和复审。中途不运行源码测试、不逐单元验收；必要编译允许，最终 Rust 格式与 release 构建仍须有证据。
- Bake **全量对账**：最终每signature每行由live重算并逐字段比较，另做确定性查询差分及best/top顺序对照。最终新语义29,783,417行/3,292兼容查询零差异、2,148按门禁跳过，CLI exit0；不把跳过算通过。旧完整表和第一轮对账仍为历史证据，最终认证来自7695096fd的隔离新表。
- 终态断言：最终用 `--assert-invariants` 批量跑（`assert_rotation_invariants.py` 已含），按 `arknights-invariant-gate` 做整体自检，再用 `arknights-intent-review` 复审。U6 的授权暖机差异逐项解释，其他差异视为未完成；旧失败与新增失败分开报告。
- **基础性能日志已实现**：在阶段边界记录计时，按批次统计次数，结束集中输出；细粒度调用标签按需开启。日志用于确认耗时分布与重复结算来源，不将实现中的计时当作逐单元验收。未实测的日志开销不声称零成本，不额外开启测量任务。
- 性能：最终运行可留存现有耗时、阶段和批次日志，区分冷启动/生成/加载与热运行。用户不要求实际性能指标，不额外开测量任务；历史单 box 测量不能代表最终收益，未测的收益与日志开销不预先声明。

## 6.1 子代理使用经验

长篇 `simple-query` 调查（宽问题、几十次工具调用）在本轮多次中途无声中断，没有最终报告也没有错误。可靠做法：每个子代理只答一个具体事实，限制 15 次工具调用，要求边查边追加写入 `/tmp/<名>/report.md`，主代理读该文件并回源码核对会改变决定的结论。

## 7. 尚未验证与非阻塞项

- U4b生产接入、U5、U6、miss分类/服务预热已实现；本轮内存和连续暖机实现、局部数值、静态闭合与最终全量/152盒包均完成，五既有预算失败保留。最终5949叶差异归因已闭合，见§8.1；历史调查和旧未完成状态不重复开工，源码提交由主代理收口。
- 冻结版本阶段归因已覆盖N/A/B、attempt、peak/γ；单box只有attempt_0，没有实际中枢重建。搜索归属15297与全局15379相差82，具体来源未归因；不能混用两个计数边界。其他box和轮换的阶段分布、最终版本收益尚未测。
- Worker是否依赖外部data运行期替换，仍是将来采用build.rs前的条件；当前已选加载期编号，**不阻塞本轮**。运行目录可写性/并发发布/失败回live仍需最终验证；无参预热、Auto 与 serve 同步 warm 均已实现，不再把旧空桩当剩余实现。
- schema17实际合法行数、文件体积、生成和冷加载开销可以记录，但不作为可接受体积裁决或推进阻塞项；完整表与加载期索引裁决保持；实际机器内存有限，现按授权降低表示与峰值内存继续，见 §0.1。
- 第一轮四轮换/full-e2、Bake对账、断言和独立复审的真实未通过结果保留在交接单§4.7；当前最终新语义包已完成，真实结果见§4.9。五预算失败没有新增/移除，raw输出仍变化；最后归因已闭合，不以缺失输出same或脚本exit替代断言/产物的真实状态。
- 控制中枢、发电、宿舍没有独立普遍热路径测量；本轮不为覆盖更多设施另起外围改造。

## 8. 已裁决事项与历史估算

已裁决（2026-10-28，用户用选项给出）：

1. 技能模型：只移植编号和定长数组，技能结算沿用 v2 的 `EffectAtom` 解释器。
2. 编号生成：先落加载期的稠密 `u16` 表，之后再看是否接 `build.rs`。接 `build.rs` 之前要先确认 Worker 部署不依赖运行期替换数据（§3.8、§7）。
3. 暖机：改成随班时长变化。这是行为变化，见 U6，放在纯重构之后。
4. `data/baked`：不提交，首次运行时生成（见 U1b）。

最新用户裁决（2026-10-28）：

> 内存、冷路径都不需要任何顾虑，我们有很强大的服务器，重要的是热路径必须要快

5. U3已裁决使用完整表和加载期索引，内存与冷路径不作为顾虑，热查询速度优先；“先量规模再决定全表还是上界/现场结算”的选择已关闭，不再问。
6. **历史估算，非现行事实**：早期枚举上限143693→482342（3.3568倍），按当时schema16行大小粗估约72.86MiB；该估算尚未包含后来保留原始elite的完整身份维度，不能代表schema17合法行数、文件体积或加载成本。最终可记录实际规模，不改变完整表裁决。


### 8.1 最终差异与暖机正确性（2026-10-29，用户已裁决继续）

用户已裁决暖机随班长变化，但原验收只允许含五暖机技能的房间差异。最终包不满足该限制：前三轮同成员132个效率变化中129含暖机，3个b-112-404无暖机房的全局/站级效率也变，实际中枢名单改变；三轮MAA变化10/3/3盒。24/24失败集合移除b-101-424、新增b-112-404（均涤火杰西卡shift2），完整输入和原比较未改。full-e2两班7处房间换人、菲亚从禁用变但书/巫恋，制造日产9.078→8.947，旧断言失败而新通过。完整原始diff见交接单§4.7的证据入口。

第一轮历史核实：公孙固定baseline 9.084→9.039是旧U6暖机差额；上述三个同成员非暖机房的房内技能效率不变，中枢改变影响全局项。后续完整旧语义消融已确认本覆盖内未发现纯性能数值、选人或MAA偏差，详见下方；最终连续暖机结果与归因已完成，详见下方；旧完整表或静态复审不替代最终验证。

用户后续明确：“暖机按正确做法来做，如果说断言有问题的话，就修改掉它。”此前保留暖机/拆开验收的二选一已关闭；不回退20小时，也不把旧名单不变当作正确性的要求。只有断言偏离真实预算/机制时才修改，真实违规仍由正确gate拒绝。

最新时间函数裁决原话：“线性增长，第1个小时加成为0。”原会话问题第二项仅问阿罗玛首小时，因此首小时0仅适用于阿罗玛；其他四技能保留首小时20%（芬/刻俄柏）或15%（克洛丝/稀音），之后按+1%或+2%每小时线性至25%。阿罗玛整个首小时0，之后+2%每小时至20%。该裁决覆盖旧阶跃和round/clamp时间口径；当前新冻结局部数值验收及指定P1/P2静态闭合已完成，最终新表与统一包已完成，归因已闭合，详见下方。

已核实的正确性事实及范围：

- **纯性能归因已在覆盖内关闭**：固定旧20h的消融二进制`2750e3ec874bdc5afb8789990886d6f85624b45e3ef313b59b1ccce5d947a08d`真实完成152盒及full-e2无断言/带断言两次执行。原比较仅剔顶层elapsed，仍为`same=5 changed=147`，不改比较规则、不声称JSON字节相同；147个batch成功输出加1个full-e2输出共296个叶差异，全部是profile的`baseline_label`/`layout_label`绝对数据路径标签，同一输入文件在main、snapshot及冻结输入的SHA256一致。成员、效率、单位产出浮点、rotation及MAA字段零差异；五例断言失败集合/原因逐字一致，full-e2无断言仅标签变化、带断言仍在shift2为涤火杰西卡失败。旧完整表29,783,417行、3,435兼容查询零差异，2,005查询跳过，CLI exit1仅消融snapshot中旧但书CSV期望。证据`correctness-attribution/ablation/verification/{report.md,summary.json,label-attribution-summary.json,nonlabel-leaf-differences.json}`。这证明本覆盖内未发现纯性能数值或选择偏差，不推广任意输入或独立Required+peer-reset行为。
- **首轮连续暖机历史数值记录**：首轮CLI `592270b93b3cf693eebd51563827e7f4d0e4ecaea01291a6dc15d89a1c00f8d9`的55×3解析、360full（288成功、72预期非有限错误）、288search及135顺序比较在点名ramp/bucket与指定心情输入下零语义差异，原50条数学日产尾差及完整证据`correctness-warm/numeric-validation.{md,json}`保留。该轮未执行后来追加的P1/P2完整技能/极短段反例；不能沿用首轮通过认证最终源码。
- **新冻结连续暖机局部验收已完成，搜索覆盖按既有政策分开**：CLI `7695096fd53c33dba66a4368c83061fa73a1676210bc28ec70c5418b8446d405`与对应core rlib起止指纹稳定。保留并重跑55×3独立解析原oracle、225原输入及首轮证据，新增129输入；总1122次full（1050成功、72预期非有限错误）与数学千分整数及公开f64合同零差异，无panic或非预期solver错误。总1050次search中798获准，经真实light→full效率断言及best/top完整字段精确比较通过；252次20/30h输入由原有`>=12h`负心情扣减技能准入政策排除，evaluated=0，不能称这些组合full/light/search一致。门禁常量在e40900f与当前均为12.0。P1阿罗玛+槐琥+铅踝mood20/shift0/Gold六序均技能50pp、final1.530；20h full为1.620，而search空结果。P2芬+槐琥`nextafter(1,+∞)`三配方/两序均full/search一致，final1.420，不再拒绝或丢组合。指定P1/P2静态复审闭合；普通段按阈值分段积分，不足两个可表示内点的微段保留完整权重、暖机解析平均及原解释器代表点求值，仍受f64分辨率与舍入约束，不能称所有实数积分精确。原50及新增120条数学日产二进制尾差另存，验收按公开f64合同精确相等、不用容差。证据`correctness-warm/p1p2-final/{report.md,summary.json,search-policy-source.json}`、`correctness-warm/p1p2-closure-review.md`；不能证明全技能/全局注入、游戏个体耗尽语义、全部微段、排班或全局最优。
- **消融到最终源码归因已核对**：main与消融snapshot按真实文件路径比对274个crates/data文件，只有7处差异：暖机函数/解释器/solver、types时长注释、Bake暖机门禁、技能表五暖机及槐琥phase顺序、但书CSV期望；没有缺失文件。内存与其他性能实现相同，证明本轮最终实现与旧语义消融的来源边界，不替代最终输出差分。证据`correctness-warm/{ablation-final-provenance.json,ablation-final-source.diff}`。
- **最终统一运行及归因已完成**：冻结CLI `7695096fd53c33dba66a4368c83061fa73a1676210bc28ec70c5418b8446d405`在`linear-warm-final/`真实完成隔离完整Bake及统一包。Bake CLI exit0，29,783,417行/3,292兼容查询差异0，skipped signatures0、queries2,148；13非空signature、674状态，CURRENT=`gen-28818-1793208859780185706-0`，header/manifest/summary核对通过，但书回归PASS。3/2/12_12_12各38完成且Rust断言/solver/input/timeout错误0；24_24为38检查/33完成/5既有预算失败，完整失败tuple逐字同基线，新增/移除0。原比较仅剔顶层elapsed，仍为same5/changed147，脚本exit1包含输出变化，不能冒充所有gate全绿。全精二无断言exit0，profile11叶/rotation10叶变化，MAA完全相同；带断言旧新均exit1，在shift2同一涤火杰西卡预算失败，均无输出。152子命令起止binary/input SHA与显式环境一致。5949叶原差分全部保留，原621机器unknown另附闭合解释；覆盖内未归因数值0、导出不一致0、新增/移除失败0。直接暖机、排序/积分机制集合及连带重选已归因闭合，不证明每次重选独立因果或任意输入，五既有预算gate仍失败。证据`linear-warm-final/{report.md,summary.json,verification}`；本轮冷路径覆盖来自完整verify/Required兼容查询及Auto加载，未新开重生成或性能测量任务。

- **最终差异归因已闭合**：5949条原叶差分与621条初始unknown原样保留，另存闭合解释，覆盖内未归因数值0、export不一致0、新增/移除失败0。115个同成员暖机制造班房（690叶）为直接数值变化，195个房间班次重选、10个同成员跨房上下文变化。批量选中组合没有槐琥分桶+铅踝扣减实际buff共同例，不能冒充批量排序覆盖；该P1由新增129输入数值包验证。唯一6叶同成员无暖机变化为17-256第一班娜斯提房：办公室宴→E0淬羽赫默，莱茵tag人数2→3，造价高昂`min(15,3×n)`正好+3pp，房间1阶技能0.96→0.99/总2.01→2.04，非匿名全局补偿。公孙profile固定baseline 9.084→9.017由独立连续暖机差额−66.5millis及既有逐班整数舍入到−67解释；该证据是暖机delta/舍入边界，不从零重算全部历史总值。全精二名单/MAA相同，仅第二班manu3阿罗玛24h平均15pp替代旧15.5pp，房间2.215→2.210、日产9.078→9.075。296个旧新rotation、744班的小计/权重/日产千分合同精确核对通过，2976制造班房扫描无槐铅实际buff对。控制消融及274文件源码差异约束把本覆盖变化归入暖机/排序/门禁机制集合，连带重选逐步因果仍为有控制证据的推断，不认证全局最优、任意输入或所有f64微段。证据`linear-warm-final/attribution-audit/{closure-report.md,closure-summary.json,closed-leaf-attribution.json}`。

- **独立终审已封口**：`correctness-warm/final-intent-review.md`对用户原话、实现边界及最终证据独立复核，未发现实质代码bug；此前证据缺项在覆盖内已闭合。联合暖机/排序/门禁机制集合归因有控制证据，不把每次重选写成独立局部因果证明；原5949严格全叶口径和621机器分类原记录均保留。

- **但书**：`reg_gsl_docus_solo`的1.907来自旧锚15910.75；当前固定来源锚15929.2对应倍率`round(15929.2/10265,3)=1.552`，纸面1.230结算得到`round(1.230×1.552,3)=1.909`。CSV期望已改为1.909，机制解释0.550、容差0.001、公式与fixture保持；旧完整Bake的exit1是历史实测，最终7695096fd回归已exit0/但书PASS。暖机源码修改会改变`None`的20h表数值和exe指纹；原binary与旧表/对账证据保留，仅证明内存实现按旧暖机语义等价。最终binary已在隔离目录重新生成完整Bake并verify，CLI exit0；不手改指纹，也不能用旧完整表验收替代新语义验收。证据`correctness-docus/report.md`。
- **24/24预算**：五例移除断言后的正常完整assignment均exit0，按终态逐班重算涤火杰西卡24h消耗分别28.80、28.80、28.80、30.00、27.60，available均24。含灵知四例即使补计固定来源恢复0.05/h，仍为27.60、27.60、27.60、28.80，第五例保持27.60，故断言均未误报，保留gate。canonical既定的正常warning fallback可以输出超预算排班，显式终态gate仍拒绝；本轮未修排班，也未证明其他账号或全局无解。证据`correctness-budget/{report.md,budget-breakdown.json}`。
- **暖机**：固定知识库没有明确阶跃/连续及阿罗玛首小时，直接PRTS来源HTTP403；旧小时段手算仅为条件性反例。最新用户裁决已补齐时间函数：阿罗玛首小时整个0，之后`min(2×(t−1),20)%`；其余四技能首小时initial 20/15%，之后+1/2%每小时、cap25%。None默认20h采用新连续语义，Some(h)使用真实时长，不round/clamp；发电不改。先平均暖机再做槐琥floor/cap与按时间先结算后平均不交换；连续积分首轮验证及尚待修正的两项复审边界见上方。证据`correctness-warm/{report.md,temporal-source.md,numeric-validation.md,numeric-validation.json}`。

上述证据根均为`target/codex-runs/perf-final-20261028/`。实现、最终隔离Bake/CLI回归、152盒/full-e2及指定P1/P2闭合均完成；最终5949叶归因已闭合，按交付状态收口，无需重新实施暖机或重复旧验证。保留普通段积分/f64微段边界、五既有预算失败、旧binary/表、第一轮全部差异与唯一恢复baseline，不手改指纹、不重开1+1/四赤金/Mower。最终版本提交由主代理记录。
