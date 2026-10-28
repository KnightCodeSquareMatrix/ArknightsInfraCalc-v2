# 性能优化调查与 Bake 重设计

> 文档角色：active-change
> 生命周期状态：proposal
> 当前真源：docs/PERFORMANCE_ENGINEERING.md；docs/QUALITY_AND_AUDIT.md
> 摘要：2026-10-28 性能优化 U1～U6 已提交实现、历史调查、冻结基线与最终验收和复审收口入口

本页是 [性能优化交接](性能优化交接.md) 的展开。交接单记录方向、对拍基线和测量；本页记录**优化前 Bake 阻塞与数据表示调查、当前实现边界和最终验收剩余工作**。性能工程的现行规则在 [PERFORMANCE_ENGINEERING](../PERFORMANCE_ENGINEERING.md)。读完本页不需要再去读 `bake.rs` 全文。

## 0. 当前进度与接手入口（2026-10-28）

> **2026-10-28 23:00 后的验收状态**：本地 WSL 全表生成触发 OOM（内核记录杀死 infra-cli），随后执行环境重启，`/tmp` 冻结基线和临时验收记录丢失，尚无全量对账或最终对拍通过证据。SSH 只读资源查询被沙箱以 `socket: Operation not permitted` 拒绝。没有缩表、缩输入或降低验收；继续路径待用户选择大内存服务器执行入口或本地冷路径分块。
> 冻结源码已由原会话日志确证：基底 `3201fa7`，仅替换为 `47cbf62` 中四个 U1a 源码 blob；原提交成功 `git apply --cached` 的正是冻结 patch。原基线 SHA256 `315c1954fd3d8c75c10152ec353398734aa0c79ee8397656099833a22b1def07`，与阶段测量 `168a12...` 分开。原二进制及产物仍丢失，当前精确源码的 release 重建/再采集进行中，不把源码恢复写成二进制字节复刻。
> 持久证据改存 `target/codex-runs/perf-final-20261028`：`baseline-recovery/` 保存源码恢复和原会话证据，`baseline-recovered/` 保存重建与采集，`intent-review/report.md` 为独立静态复审（未发现实质问题，不能代替运行验收），`bake-foreground/kernel-oom-session-evidence.json` 保存内核 OOM 原工具输出。此处后续状态优先于本页历史 `/tmp` 路径。

- **U1～U6 均已实现并提交，代码 HEAD 为 509ad44；最终运行验收与独立复审仍待收口。** 格式化/编译证据不能替代全量对账、输出等价或性能收益证据。
- **U1/U2/U3**：U1a 命中字段补全和全量行/查询差分入口（47cbf62）、U1b Auto 首次再生、exe 指纹及失败缓存、generation/CURRENT 原子发布（3ef5f6c）已完成；生成物取消跟踪和 schema4 旧目录清理（dc2af4c）已完成。加载期编号/原子复用（af86268）、贸易/制造借用归并（20fc4d2/e127b28）、房间资源 16 槽数组（458fb33）已完成。完整技能身份（e42f9d0）、真实 pool builder 生成完整变体并全覆盖核对（528b9ea）、schema17 colex rank 直查和 live top-K 比较（2caf24d）已完成。
- **U4 memo 生产接入完成**：Exact 编码（5be9614）、请求 scope/完整叶键/精确 intern/Arc 保活（19c9d1a）、贸易制造搜索接入与命中成本汇总（c29b2b2）、完整排班请求复用（5b165f1）均已提交。命中和减少结算的实际情况由最终日志确认。
- **U5 完成**：制造组合借用和轻量结算（fe53d02）、配方注入缓存（c413336）、贸易借用视图/身份判定/生产搜索贯通（022563c/e421877/9f5fde1）、共用平铺枚举与泛型评估闭包（45dc1fe）已提交。Bake 固定签名槽与生成目录隔离（6a22e21）、miss 分类/冷阶段日志和服务同步预热（2880cc6）也已完成。
- **U6 完成（509ad44）**：制造暖机按整班时长的逐时平均贡献进入解释器，None 保持 20 小时；心情分段共用整班窗口，未实现非线性积分。含 AddEffRamp 的制造查询池在 Some(h) 下整体回 live，None 仍用固定表；Required 经统一公开门禁严格报 miss，制造 peer-reset 外层绕过路径已删除，CLI 同语义。详见 §4.3。
- **唯一冻结 baseline**：`/tmp/perf-current-20261028` 保存冻结二进制、工作树差异与指纹；四轮换均 38 盒，rot3/rot2/rot12_12_12 成功，rot24_24 保留 5 个已知断言失败。全精二 `full-e2-output` 无断言产物成功，`full-e2` 带断言保留同类失败。详见交接单 §4.1；不在 U6 前另建基线，旧 e976b1a 仅历史参考。
- **必要剩余工作**：汇总最终 release/格式证据、Bake 全量行和查询差分、四轮换与全精二产物/终态断言，处理差异并完成独立复审收口。最终验收和复审进行中，不能写通过；只授权暖机房差异，若连带选人或排班差异超出授权范围，原样报告，不放宽验收。
- 基础汇总日志（4d057a3）和批次/阶段归因（65355b2）已完成；冻结版本单 box 三次测量只作历史归因证据。用户不要求实际性能指标，日志可随最终运行留存，不额外开启性能测量任务。
- 删除源码/Python 测试及无引用 fixture、CI 和文档清理（0d8b935）、泛型轮换入口及相关文案已完成。CRLF/LF、default_dorm_beds 和两条可见性 warning 不处理；box-bundles 已复制但不入库。1+1 和四赤金本轮不开工；共享工作区单 writer，git 操作由主代理负责。

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

- 本轮 baseline：`/tmp/perf-current-20261028` 的冻结二进制与采集产物（冻结时含未提交U1a）；四轮换及全精二产物/断言状态见交接单§4.1，不用后续HEAD代替。旧 `/tmp/perf-baseline/infra-cli-baseline`（e976b1a）仅保留历史参考，不用于本轮最终验收。`/tmp` 重启会丢，需保留冻结版本与构建/输入指纹才能重建。
- box 集合 `data/fixtures/box-bundles/`（`.gitignore` 忽略，已复制进 main 工作树；38 个）。
- 最终对拍命令形态：`python3 scripts/assert_rotation_invariants.py --group all --rotation 3 --binary target/release/infra-cli --skip-build --jobs 8 --output-dir /tmp/perf-new/rot3 --compare-to <当前冻结baseline的rot3目录>`；`--rotation` 依次用 `3`、`2`、`12/12/12`、`24/24`，对应 `/tmp/perf-current-20261028` 实际生成的四种轮换目录。只在当前版本与最终版本各跑一轮完整包。
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
- 解释器保留 v2 的 EffectAtom 语义；U2/U5 复用编号、借用组合数据、轻量结算与平铺枚举。U4 Exact 编码和完整叶键已接贸易/制造及完整排班请求 scope，memo 已在生产调用中生效；实际复用和输出等价仍待最终包确认。
- 贸易hit的日产字段用单位值与total_efficiency按24小时恢复，**不得按shift_hours/24再缩放**；只有`TradeResult.daily_at_shift`表示实际班次量并缩放。制造暖机 Some(h) 已按整班窗口计算平均贡献，None 保持 20 小时；仅授权暖机房差异，见 §4.3。

Bake层拥有存储完整性、上下文准入与live回退；搜索层拥有枚举、合法过滤、比较与输出合同。尚未完成最终全量对账与查询差分，不提前把静态判断或编译通过声明成普遍等价。

### 4.2 保证等级声明

- Bake 键完整且表由同一个 live solver 生成，查表结果与 live 逐字段一致：**safe reduction**。声称成立的前提是 §6 的全量对账通过。
- 未覆盖的上下文通过Bake门禁回live；有限正心情与班时长按现行字段口径处理；Some(h) 且池含暖机时整体回 live，None 保持固定表。门禁与现场结算不改变候选集合，不是剪枝。
- 任何“不安全剪枝、更大枚举”（用户愿意接受的方向）：这一轮**不做**；一旦做，须标 heuristic 或 approximation，并按质量文档处理，不得声称全局最优。

### 4.3 工作单元（实现结果）

各单元可独立提交，由主代理负责 git 写操作；中途不逐单元对拍或验收，必要编译允许。代码已完成，最终统一包按 §6 验收。U1～U5 必须保持结果，U6 仅允许授权暖机行为差异。

- **U1a 已实现：命中补全与全量对账入口**：持久化/恢复完整命中数值，行verify重算完整响应并做确定性查询差分；制造产出字段和live排序口径已补齐。最终运行校验未做，schema16旧结果由schema17重建。
- **U1b 已实现：运行时再生与发布**：Auto缺表/失效一次Disabled live生成，exe指纹及失败缓存，验证后独立generation与CURRENT原子发布；Required严格只读，CLI/debug共用解析。目录清理已完成；固定 BakeMissKind 分类、query_errors 与冷阶段 calls/errors/nanos 汇总及 serve 同步预热均已提交（2880cc6），Required 严格只读。
- **U2 已实现：加载期编号、借用与定长资源槽**：干员/buff稠密编号，建池预编译复用，贸易/制造借用输入与稳定归并，贸易/制造房间资源16槽数组。GlobalResourcePool与中枢/办公室/发电快照保留现有owner边界；不扩展候选准入与规则语义。
- **U3 已实现：完整变体存储与rank直查**：身份为name/原始elite/ordered buff_ids/tags；全部实例设施绑定经真实pool builder生成E0/E1 tier_0、低星等级解锁、E1/E2 tier_up五类代表练度，按完整身份去重，制造各配方使用同一完整变体池，同名组合互斥。完整legal combo集合冷校验，每signature建立checked dense colex rank数组；热查询沿live原枚举次序直查，有界堆按贸易效率/枚举序与制造效率/仓库/组合姓名比较，只物化best/top，evaluated保留全部合法过滤组合。完整layoutbaseline及技能原文/compiled atom身份门禁仍在，未覆盖动态或心情敏感条件回live。贸易 hit 日产不按 shift 再缩放；制造表生成和行 verify 均以 None 的 20 小时暖机值重算，Some(h) 且池含 AddEffRamp 时整体回 live。Bake 物理 signature 使用固定槽，不在热路径拼字符串；可用 `ARKNIGHTS_INFRA_BAKED_DIR` 显式隔离生成目录（6a22e21），默认目录发现顺序保持原规则。完整表和加载期索引已裁决，不因体积/冷启动重新请求选择。
- **U4a/U4b 已实现并接生产**：Exact 编码（5be9614）、TLS 请求 scope/完整叶键/精确字节 intern/Arc 保活（19c9d1a）、贸易制造叶搜索及成本汇总（c29b2b2）、完整排班请求复用（5b165f1）已提交。键覆盖、嵌套生命周期、返回顺序和输出等价由最终包验证；不把“trade_solves 接近组合数”写成保证。
- **U5 已实现：搜索与结算解耦**：共用平铺组合枚举接受泛型评估闭包（45dc1fe）；制造借用池数据与轻量结算（fe53d02）和配方注入缓存（c413336）、贸易房间借用视图及 shortcut/链段/搜索贯通（022563c/e421877/9f5fde1）均已提交。枚举顺序、合法过滤与 live 比较口径保留；本单元未增加不安全剪枝。
- **U6 已实现：暖机随整班时长变化（509ad44）**：有限的 `Some(h)` 先 `round` 再 `clamp(1, 24)`，暖机贡献取该窗口内逐时加成的算术平均，然后进入现有阶段解释器；`None` 保持 20 小时。非有限时长拒绝；`layout/assign` 默认 24 小时也会使用 24 小时平均。心情分段重解共用整班暖机窗口，未做逐时非线性积分，不能据此声称精确积分语义。
- **U6 Bake 门禁**：`Some(h)` 且整个制造候选池含任一 `AddEffRamp` 时，查询整体回 live；`None` 固定表仍按 20 小时生成/verify。公开 `try_baked_*` 统一判定；Required 对暖机、peer-reset 等 miss 严格报错，制造 peer-reset 外层绕过路径已删除；CLI 使用同一语义。
- **最终差异边界**：最终版本统一与 `/tmp/perf-current-20261028` 比较，不在 U6 前另建 baseline。只允许含这五个暖机技能（急性子、“等不及”、慢性子、延时摄影、例行清扫）的房间产生授权差异，并逐项解释；若引起其他选择或排班差异，按超范围差异原样报告，不自行放宽。

### 4.4 移植 v3 时的取舍

- 移植的是**机制**：编号化、定长数组、临时空间复用、搜索与结算解耦。不移植 v3 的规则行为，也不移植它的函数指针式技能模型。
- 技能结算仍沿用 v2 的 `EffectAtom` 解释器，避免丢失 v2 保留的规则语义（用户 2026-10-28：v3 丢了很多规则设计，v2 是语义参考和对拍基准）。

## 5. 不做什么

- 不为单一 box、房间号、班次下标写补偿特判。
- 不放宽对拍：任何产物差异（除用户裁决过的）都视为未完成。
- 不碰 Mower 动态换班、四赤金、1+1 自动轮换（交接单已排在性能之后）。
- 不把 v3 AGENTS.md 里“禁止 Bake”的规则带进 v2。

## 6. 验收与基础性能日志

- **当前版本与最终版本一次统一对拍**：使用 `/tmp/perf-current-20261028` 的冻结二进制与产物（冻结时含未提交U1a）作为 baseline；U1a～U6 已实现，最终统一跑 §2 四种轮换全量对拍、全精二产物与已知断言失败样本、Bake 全量对账、终态断言和复审。中途不运行源码测试、不逐单元验收；必要编译允许，最终 Rust 格式与 release 构建仍须有证据。
- Bake **全量对账**：最终对每个 signature 的每一行用 live solver 重算并逐字段比较，另做确定性查询差分，比较 best 与 top_k 顺序。旧首/中/尾抽样不足以支撑 safe reduction；全量入口已实现但最终运行尚未完成，全量覆盖与实际开销须以运行证据确认，不用旧生成耗时推断对账便宜。
- 终态断言：最终用 `--assert-invariants` 批量跑（`assert_rotation_invariants.py` 已含），按 `arknights-invariant-gate` 做整体自检，再用 `arknights-intent-review` 复审。U6 的授权暖机差异逐项解释，其他差异视为未完成；旧失败与新增失败分开报告。
- **基础性能日志已实现**：在阶段边界记录计时，按批次统计次数，结束集中输出；细粒度调用标签按需开启。日志用于确认耗时分布与重复结算来源，不将实现中的计时当作逐单元验收。未实测的日志开销不声称零成本，不额外开启测量任务。
- 性能：最终运行可留存现有耗时、阶段和批次日志，区分冷启动/生成/加载与热运行。用户不要求实际性能指标，不额外开测量任务；历史单 box 测量不能代表最终收益，未测的收益与日志开销不预先声明。

## 6.1 子代理使用经验

长篇 `simple-query` 调查（宽问题、几十次工具调用）在本轮多次中途无声中断，没有最终报告也没有错误。可靠做法：每个子代理只答一个具体事实，限制 15 次工具调用，要求边查边追加写入 `/tmp/<名>/report.md`，主代理读该文件并回源码核对会改变决定的结论。

## 7. 尚未验证与非阻塞项

- U4b 生产接入、U5、U6、miss 分类和服务预热均已实现并提交；键覆盖、嵌套/请求生命周期、返回顺序、实际计数与输出等价仍须由最终包确认。必要剩余项是运行验收、差异处理和独立复审收口。
- 冻结版本阶段归因已覆盖N/A/B、attempt、peak/γ；单box只有attempt_0，没有实际中枢重建。搜索归属15297与全局15379相差82，具体来源未归因；不能混用两个计数边界。其他box和轮换的阶段分布、最终版本收益尚未测。
- Worker是否依赖外部data运行期替换，仍是将来采用build.rs前的条件；当前已选加载期编号，**不阻塞本轮**。运行目录可写性/并发发布/失败回live仍需最终验证；无参预热、Auto 与 serve 同步 warm 均已实现，不再把旧空桩当剩余实现。
- schema17实际合法行数、文件体积、生成和冷加载开销可以记录，但不作为可接受体积裁决或推进阻塞项；用户已授权内存/冷路径无限制、完整表与加载期索引。
- 最终统一四轮换与全精二包、Bake 全量行/查询差分、终态断言和独立复审正在进行，结果尚未收口。全精二无断言产物成功与带断言已知失败分开报告；暖机授权差异逐项解释，其他差异不放宽。
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
