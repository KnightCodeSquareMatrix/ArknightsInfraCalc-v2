# 性能优化调查与 Bake 重设计

> 文档角色：active-change
> 生命周期状态：proposal
> 当前真源：docs/PERFORMANCE_ENGINEERING.md；docs/QUALITY_AND_AUDIT.md
> 摘要：2026-10-28 性能优化的调查结果（已核实事实与出处）、Bake 与定长表的重设计、工作单元划分、验收方式和待裁决项，供下一个 agent 少读代码直接接手

本页是 [性能优化交接](性能优化交接.md) 的展开。交接单记录方向、对拍基线和测量；本页记录**为什么 Bake 从不命中、v2 与 v3 的数据表示差在哪、怎么改、怎么验收**。性能工程的现行规则在 [PERFORMANCE_ENGINEERING](../PERFORMANCE_ENGINEERING.md)。读完本页不需要再去读 `bake.rs` 全文。

## 1. 用户裁决（2026-10-28，原意）

- 目标：优化到非常快，少写手工规则，可以做不安全的剪枝、较大的枚举，并用 Bake 省重复任务；“不需要在意本项目的算法设计，一切只为了性能和结果”。
- 只做最终一次对拍，开发中不逐步检查。结果仍须正确。
- Bake 可以重新设计。**心情除铅踝外不影响真正效率**；暖机角色的效率可以额外写函数决定；即使 Bake 不能覆盖铅踝和暖机技能，也只需要为暖机角色做热路径计算。
- v3 的预编译（`build.rs` 编号化）和定长表要移植到 v2；v3 的单房结算写法与 v2 不同，可以节省时间。
- 当前裁决足以支持改完 Bake 和定长数组的效率计算。进入无人值守：不需要用户裁决的部分尽量直接改，需要裁决的统一写文档（§8）。
- 用户认为 Bake 修复和修改干员技能表是低危。**纠正**：语义风险低（前提是输出逐字节不变），但改动面是全部解释器与全部房间搜索，必须靠 §6 的对拍验收，不能凭“低危”省掉。

## 2. 对拍与测量入口（已建好）

详见交接单 §4.1。要点：

- 基线二进制 `/tmp/perf-baseline/infra-cli-baseline`（`/tmp` 重启会丢，需要时在提交 e976b1a 构建 release 重新得到）。
- box 集合 `data/fixtures/box-bundles/`（`.gitignore` 忽略，已复制进 main 工作树；38 个）。
- 对拍命令：`python3 scripts/assert_rotation_invariants.py --group all --rotation 3 --binary target/release/infra-cli --skip-build --jobs 8 --output-dir /tmp/perf-new/rot3 --compare-to /tmp/perf-baseline/runs/rot3-a`；`--rotation` 依次用 `3`、`2`、`12/12/12`、`24/24`，对应基线目录 `rot3-a`、`rot2`、`rot12_12_12`、`rot24_24`。
- 基线里 `24/24` 有 5 个 box 因涤火杰西卡的 `main_backup_24_24_work_budget` 断言失败，改动后应以同样原因失败。
- 现有 38 个 box 都含非精二干员，碰不到 Bake 命中路径。U1b 之后要额外对 `data/fixtures/243/operbox_full_e2.json`（全精二）用 `--rotation 24/24` 做新旧对拍，它才会走到命中路径。
- 单 box 耗时：`target/release/infra-cli profile layout-full --operbox <box> --runs 1`。

## 3. 已核实的事实

主代理回源码或原始输出核对过的标 ✔。

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
- 贸易站 `shift_hours` 只把产出按 `shift_hours/24` 线性缩放（`trade/unit_output.rs:98-116`），不改纸面效率，可在查表后用函数算。

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

首次违规处（即现在放行错误状态的 owner）：

- Bake 的**键**只覆盖了“精二、心情 24、24 小时、基线 layout”这一种上下文，其余上下文一律 miss。键需要改成完整的上下文签名。
- Bake 的**有效性判断**绑在 exe 字节上，而不是绑在输入数据上。
- 解释器每组合分配（克隆、`HashMap`、`Vec<String>`），把本该常数的开销做成了线性。

修复后的责任边界：Bake 层只负责“给定完整上下文签名，返回与 live 逐字段一致的结算”；搜索层只负责枚举与剪枝；心情、班时长、铅踝、`mood<=0` 这些不进键的因素由 Bake 层之后的**独立函数**现场结算，不在表里。

完成时要删掉的冲突路径：`aic-baked-save/` 下 schema 4 的旧 JSON 目录；绑定 exe 字节的生成器指纹；`BakeMode` 里无人构造的 `Required` 之外的重复入口（如保留，需有构造点）；贸易旧的非预编译解释器路径（`collect_atoms` + `apply_atoms_loop`，在制造也走预编译之后）。

### 4.2 保证等级声明

- Bake 键完整且表由同一个 live solver 生成，查表结果与 live 逐字段一致：**safe reduction**。声称成立的前提是 §6 的全量对账通过。
- 不进键的因素（心情、班时长、铅踝、`mood<=0`）：**hard constraint 之外的现场结算**，不是剪枝。
- 任何“不安全剪枝、更大枚举”（用户愿意接受的方向）：这一轮**不做**；一旦做，须标 heuristic 或 approximation，并按质量文档处理，不得声称全局最优。

### 4.3 工作单元（建议顺序）

每个单元独立提交，提交前跑 §2 的对拍；输出必须逐项不变。

- **U1a 命中结果补全与全量对账（先做，不改变生产输出）**：现有 Bake 命中结果与 live 不是逐字段一致（§3.7：制造的 `unit_output_per_day`/`output_per_day` 为 0，排序末级键不同等）。在让表“生效”之前，先让 `row_to_trade_hit` 与制造对应函数产出的命中结构和 live 的 `eval_combo_hit` 逐字段相等（缺的字段要么存进行里，要么用函数从行数据重算），排序改成 live 的比较规则；再把 `bake verify` 扩成**全量对账**：对每个 signature 的每一行，用 live 搜索路径重建同一组合的命中结构并整体比较，任何不一致都使生成或校验失败并列出前若干条。因为现在表在生产里从不生效，这一步不改变任何输出。
- **U1b 让表生效、可见、可再生（依赖 U1a）**：
  - 指纹**保留可执行文件哈希**。上一版文档写的“改成输入内容哈希”作废：表现在不再提交，而是首次运行时按当前构建生成，exe 哈希正好防住“求解器逻辑改了但表没重算”，只看输入文件会漏掉这种情况。改动只有：指纹在进程内只算一次，失败结果同样缓存。
  - 表缺失或失效时（`BakeMode::Auto`），先置位 `RUNTIME_BAKE_IN_PROGRESS`，用 live solver 生成到临时目录，校验通过后原子改名到目标；并发进程竞争时，输的一方发现目标已有效就丢弃自己的结果。目标目录不可写则本进程放弃 Bake、回退 live，并记录原因。
  - miss 原因按类别计数，输出在 `[perf]` 汇总里，不再静默。
  - `data/baked/` 加入 `.gitignore`，`git rm --cached` 取消跟踪 12 个文件（历史里仍可找回），schema 4 旧目录 `aic-baked-save/` 删除。
  - **风险**：表一旦生效，满足条件的查询（例如全精二 box 的 24/24 主班）会真正命中；若 U1a 没做完，不完整的命中结果会进入产出。所以 U1b 必须在 U1a 之后，且对拍要加入 `data/fixtures/243/operbox_full_e2.json`（现有 38 个 box 都含非精二干员，碰不到命中路径）。
- **U2 编号化与定长表**：为干员与 buff 建稠密 `u16` 编号，热路径用数组下标代替字符串与 `HashMap`。具体生成方式（`build.rs` 或加载期）取决于 §3.8 的结论与 §8-2。先做贸易：`TradeContext::from_room` 去掉克隆，`current_room_tag_counts` 改定长数组；再把制造的原子展开挪到建池时预编译，与贸易对齐，最后删旧路径。
- **U3 Bake 键重设计**：键 = 房间类型×等级×订单/配方×gold_lines×**每个干员的技能状态变体**（由精英阶段与等级决定生效的是 tier_0 还是 tier_up，参考 v3 的 `OPERATOR_SKILL_VARIANTS`）×layout 签名；心情与班时长移出键。生成器要对“心情无关”做一次**自检**：同一批行在心情 24 与另一个心情下各算一遍，除含铅踝的行外必须逐字段相等，不相等则生成失败（这是生成期的一次性断言，不是运行期逐步检查）。查询侧：心情 ≤ 0 → live；含铅踝（`buff_id` 命中那两个技能）→ live 或专用函数；贸易 `shift_hours` → 查表后按 `shift_hours/24` 缩放；制造含暖机技能 → 表里存 20 小时平均值（与现行 live 一致）。表规模随变体维度增长，须先量行数再决定存全表还是只存上界加现场结算。
- **U4 排班重复结算**：依据 §3.7 的结论，对同一（池签名，订单，等级，gold_lines，上下文）的搜索结果做记忆化，目标是把 `trade_solves` 压到接近组合数。
- **U5 搜索与结算解耦**：参考 v3 `search_room_combo`，枚举层接受评估闭包，为后续“大枚举”做准备。这一步改变结构不改变结果，放最后。
- **U6 暖机随班时长变化（行为变化，最后做）**：用户裁决（2026-10-28）：暖机技能效率随实际班时长变化。现状制造站固定取 20 小时平均。新规则（动手前再核对一次的默认假设）：`shift_hours` 为 `Some(h)` 时，暖机贡献取 `h` 小时（四舍五入，至少 1，最多 24）的逐时平均；`None` 保持 20 小时。注意 `layout/assign` 默认传 24 小时，所以 24 小时班的结果也会从 20 小时平均变成 24 小时平均。需要把班时长传进制造解释器（目前只有 `solve_manufacture_with_shift` 知道）。表里含暖机技能的组合不能存固定 20 小时的值：默认含暖机技能的组合走 live，或存不含暖机项的部分再用函数补上；后者要先验证暖机项在后续阶段不被非线性使用。验收不是“逐字节不变”：U1～U5 全部完成后重新建基线，U6 之后差异只允许出现在含这五个暖机技能（急性子、“等不及”、慢性子、延时摄影、例行清扫）的房间，并逐项解释。

### 4.4 移植 v3 时的取舍

- 移植的是**机制**：编号化、定长数组、临时空间复用、搜索与结算解耦。不移植 v3 的规则行为，也不移植它的函数指针式技能模型。
- 技能结算仍沿用 v2 的 `EffectAtom` 解释器，避免丢失 v2 保留的规则语义（用户 2026-10-28：v3 丢了很多规则设计，v2 是语义参考和对拍基准）。

## 5. 不做什么

- 不为单一 box、房间号、班次下标写补偿特判。
- 不放宽对拍：任何产物差异（除用户裁决过的）都视为未完成。
- 不碰 Mower 动态换班、四赤金、1+1 自动轮换（交接单已排在性能之后）。
- 不把 v3 AGENTS.md 里“禁止 Bake”的规则带进 v2。

## 6. 验收

- 每个单元：`cargo fmt --all`、`cargo build --release -p infra-cli`、§2 的四种 `--rotation` 全量对拍，退出码为 0 且 `changed=0`。
- Bake 相关单元额外要求**全量对账**：生成后对每个 signature 的每一行用 live solver 重算并逐字段比较（总行数十万量级，生成本身不到 1 秒，全量比抽样便宜）。现有 `bake verify` 只对 signature 的首/中/尾抽样，不足以支撑 safe reduction 的声明，需要扩成全量。
- 整个功能完成后：`--assert-invariants` 批量跑（`assert_rotation_invariants.py` 已含），然后用 `arknights-invariant-gate` 做整体自检，再用 `arknights-intent-review` 复审。
- 性能：用 `profile layout-full` 在同一批 box 上记录前后耗时与 `trade_solves`，写回交接单 §4。

## 6.1 子代理使用经验

长篇 `simple-query` 调查（宽问题、几十次工具调用）在本轮多次中途无声中断，没有最终报告也没有错误。可靠做法：每个子代理只答一个具体事实，限制 15 次工具调用，要求边查边追加写入 `/tmp/<名>/report.md`，主代理读该文件并回源码核对会改变决定的结论。

## 7. 未核实项

- 排班阶段 15379 次结算在“候选数×中枢重建×peak/γ/源石/替补”各层的具体占比：需要在 `record_trade_solve`（`profile.rs`）处按调用阶段打标签后再跑一次 `profile layout-full`。这是 U4 开工前的第一步。
- Worker 部署（`serve`）是否带外部 `data/` 目录并依赖运行期替换，决定能否用 `build.rs`。
- Worker 部署的 `data/` 目录是否可写：U1b 的“首次运行生成”需要可写目录；`serve` 里的 `spawn_background_bake` 与 `warm_runtime_baked_table` 目前是空桩（`serve.rs:325-331`），没有可复用的实现。
- 只在一个 box（b-008-411，三班）上测过耗时分布，其他 box 与其他 `--rotation` 没测。
- 贸易单订单 live 搜索对 `breakdown` 的赋值范围、双订单搜索的 `gold_names` 等字段在 Bake 命中后如何补全，需要在 U3 开工时逐字段对照 `eval_combo_hit`（`search/trade.rs:521-592`）。
- U3 引入变体维度后的表规模、是否会超出可接受的体积。
- 控制中枢、发电、宿舍等其他设施的热路径开销没有测。

## 8. 裁决记录与待裁决

已裁决（2026-10-28，用户用选项给出）：

1. 技能模型：只移植编号和定长数组，技能结算沿用 v2 的 `EffectAtom` 解释器。
2. 编号生成：先落加载期的稠密 `u16` 表，之后再看是否接 `build.rs`。接 `build.rs` 之前要先确认 Worker 部署不依赖运行期替换数据（§3.8、§7）。
3. 暖机：改成随班时长变化。这是行为变化，见 U6，放在纯重构之后。
4. `data/baked`：不提交，首次运行时生成（见 U1b）。

待裁决：

5. U3 的表规模策略：每个干员加入技能状态变体后，行数可能大幅增加。需要先量；若过大，改为“表只存上界加现场结算”，这会降低查表的直接收益。量完后若需要取舍，再请用户定。
