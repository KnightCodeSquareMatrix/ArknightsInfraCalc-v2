# 质量、求解保证与验证证据总则

> 文档角色：canonical
> 生命周期状态：current
> 领域键：workflow.quality
> 当前真源：self
> 摘要：裁决质量、求解保证和交付证据要求

> 实现快照：Current。
> 责任：本文件是风险分层验证、求解保证、完成证明、失败基线和 Bake 安全的唯一文字真源。
> 边界：不裁决领域业务语义；业务规则以用户裁决和对应领域 Markdown 为准。

## 1. 入口与适用范围

- 普通 bug 和结果不对：按根 `AGENTS.md` 处理；复现入口不清时再读 [MAINTENANCE_MODE.md](MAINTENANCE_MODE.md) 对应章节。
- 体系 / 编排正式审计：只在 `formal-audit` 模式读取 [SYSTEM_AUDIT_WORKFLOW.md](SYSTEM_AUDIT_WORKFLOW.md)。
- 当前代码与命令事实不清时，定向读取 [PROJECT_MAP.md](../PROJECT_MAP.md)；领域文档未知时由 [INDEX.md](../INDEX.md) 路由。
- 命令执行和 manifest：使用 [scripts/codex/README.md](../../scripts/codex/README.md)。

本文件不复制四项体系审计、完整生命周期问题表或普通 bug 复现命令；这些内容分别由 system audit 和 maintenance 文档负责。

## 2. 业务语义边界

验证只能证明命令、输入、输出和证据的一致性，不能用代码行为、旧测试、fixture、历史输出或脚本结果推翻领域 Markdown。两个当前领域 Markdown 冲突时，任务状态必须为 `blocked`，等待用户裁决；不能选择一份继续实现。

### 2.1 搜索空间与保证等级

任何会改变 eligibility、候选生成、剪枝、分解、目标、top-K、Bake 或 cache 的修改必须声明保证等级：

| 等级 | 含义 | 完整搜索中的要求 |
|---|---|---|
| `hard_constraint` | 违反 canonical 领域语义，候选非法 | 记录规则、输入事实和违规 witness |
| `safe_reduction` | dominance、symmetry 或 bound 保证不删除最优解 | 说明适用前提、保持的完整目标和组合兼容性 |
| `search_heuristic` | 只改变搜索顺序或资源分配 | exact 路径必须能恢复完整候选 |
| `policy_restriction` | 用户具名选择限制空间或改变取舍 | 结果只对该 policy 范围作保证 |
| `approximation` | 为时间 / 内存主动放弃完整性 | 报告停止原因、fallback 和降低后的结果状态 |

不能从当前 top hit、priority、白名单或常见组合反推 hard constraint。无法证明降维安全时，不得声称对 canonical 完整空间全局最优。

建议结果状态区分：

- `EXACT_OPTIMAL`：对当前 canonical 模型和目标已证明最优；
- `POLICY_OPTIMAL`：只对具名 policy 限制后的空间已证明最优；
- `BEST_FOUND`：当前最好可行解，未证明最优；
- `UNKNOWN` / `TIME_LIMIT`：搜索未完成；
- `INFEASIBLE`：已证明 canonical 模型无解；
- `NO_CANDIDATE_UNDER_POLICY`：当前策略未找到，不能冒充无解。

多目标优先显式词典序或分阶段优化。匿名加权只有在高层权重严格覆盖全部低层可能贡献时才等价。

### 2.2 风险分层验证

| 改动类型 | 最低证明 |
|---|---|
| 文案、展示、纯 export 字段 | 格式 / 结构、export fidelity、对应入口 |
| owner 内数据或局部逻辑 | 最小反例、相邻反例、受影响真实入口 |
| hard constraint / eligibility | 激活、拒绝、边界反例、最终可行性 |
| objective / tie-break | 分量、边界、等价最优处理 |
| safe reduction / pruning | 关闭 reduction 的差分、小实例 oracle 或等价证明、组合规则反例 |
| decomposition / candidate generation | 小实例全局对照、跨域反例、候选完整性 |
| cache / Bake / performance | fresh-vs-cache、fallback、benchmark、保证状态 |
| Team / Shift / export | 分组与状态不变量、真实 `plan` / MAA fidelity |
| 大型 quality-refactor | old/new differential、旧路径删除、迁移与收益基线 |

低风险文档或 owner-local 修复不强制运行所有全局类别；高风险搜索空间变化不能只靠一个 golden snapshot。

### 2.3 无人值守修复与用户裁决

用户明确授权某个任务无人值守修复时，workflow 默认持续推进：实现、格式或构建失败、final-state assertion / batch 失败、以及有 canonical 路径与最小反例支撑的 reviewer finding，均可自动形成一个新的单 writer 修复单元。每个单元完成后必须重新冻结 diff、changed paths、assertion 输出和 batch 结果；旧轮证据不得用于新轮通过。

无人值守不授予业务语义裁决权。主 Agent 只编排、核对原始证据和决定继续 / 暂停；实现 Agent 不得自行放宽验收条件、删除失败断言、缩小 batch 输入、修改 canonical 语义或将未验证结果标记为通过。已批准任务明确要求新增或调整 final-state assertion 时，assertion diff 仍须按风险走独立复审。

以下是唯一的固定暂停原因，必须写为 `PAUSE_REASON`，并附直接证据；“不确定”“想扩大重构”或普通失败不是暂停理由：

| `PAUSE_REASON` | 必须暂停的条件 |
|---|---|
| `CANONICAL_CONFLICT_OR_GAP` | 两份 current canonical 冲突，或现有用户裁决 / canonical 都不足以唯一决定领域语义。 |
| `GUARANTEE_CLASS_CHANGE` | 修复会改变 hard constraint、safe reduction、heuristic、policy restriction 或 approximation 的分类、承诺范围或 fallback。 |
| `DOMAIN_POLICY_CHANGE` | 修复必须改变用户可见的机制、优先级、无解 / fallback 策略、连续在岗、reserve / bind 或其他领域政策。 |
| `UNRESOLVED_EVIDENCE_DISPUTE` | 独立 reviewer 的实质分歧无法由 canonical、最终状态 assertion、真实入口或 batch 结果裁决。 |
| `EXTERNAL_OR_IRREVERSIBLE_EFFECT` | 修复将部署、迁移 / 删除数据、覆盖用户工作区改动、改变公共接口，或产生其他不可逆外部副作用。 |

自动循环的任务包必须声明：目标及可观察的完成条件、用户已授权范围、canonical 路径、允许 changed paths、保证等级、required proof obligations、最大轮数和外部动作策略。完成条件先于实施方案，局部修复可由已确认的复现及其验收表达；任务完成后不自动开启旁支。默认上限是同一 finding 两次自动修复、整个任务四个修复单元；达到任一上限时以 `UNRESOLVED_EVIDENCE_DISPUTE` 暂停并保留 checkpoint，不得无界重试。每轮 final status 只能是：

```text
AUTO_STATUS: CONTINUE | PAUSED_FOR_USER | COMPLETED
PAUSE_REASON: <one enum above, only when paused>
FROZEN_PACKET_SHA256:
EVIDENCE_REFS:
NEXT_IMPLEMENTATION_UNIT:
```

`COMPLETED` 仍要求该风险等级的全部 gate 和独立复审通过；reviewer 的 `APPROVED`、单个命令成功或实现 Agent 的总结均不能单独关闭任务。当前 harness 尚未实现持久 writer lease、自动重试或用户通知 runtime；本节先定义所有 runtime 必须遵守的决策合同，不声称这些能力已落地。

### 2.4 初始ETA与后续心情诊断（2026-10-31）

2026-10-31 用户最新裁决：只保留最初 ETA roster 试算；后续搜索、组合、补位、逐班和动作候选不再按工作心情预算筛选或拒绝。心情净消耗、暖机与心情差积分仍按真实状态结算；终态心情不足保留质量 warning。显式 gate 核对该 warning 与实际终态是否一致，结构、Plan、绑定和动作事实仍独立验收。本次删除属于 policy，不是 safe reduction，不证明全局最优。

最初试算覆盖已有中枢名单与初始Plan中枢认领；2026-11-05按用户要求补齐非中枢正常技能与体系选人入口，使用已有owner/ABC Peak窗口。用户明确已有检查不改、Fallback例外保留。设施资格在候选构造前冻结，后续组合不复筛。批量gate通过表示所列结构与诊断准确性通过，不表示各盒心情已满足、真实宿舍回满或全局最优。历史冻结验收的心情预算失败及>=12h政策排除仍按当时基线保留，不代表当前筛选。

## 3. 回归分层

回归应按责任半径分层，不用单个大快照代替全部证明。

### 3.1 数据与机制单元层

- `skill_table.id == buff_id` 和 operator instance 归属。
- Selector、Condition、Action、Phase 和 family 合并规则。
- 0 / 1 / 多消费者、阈值边界、同房与跨站反例。
- shortcut 命中与拒绝条件。

### 3.2 单站 solver 层

- 指定房间成员的最终效率和分量。
- 普通候选与 shortcut 候选使用同一最终排序口径。
- 候选自身必须进入 `base_workforce` / facility workforce 投影，并按姓名去重。

### 3.3 Layout 生命周期层

- 激活、关闭、anchor 进编、禁止替代和跨房可见性。
- 顺序搜索必须让后房看到前房已提交成员。
- 最终 assignment 完成后刷新所有受跨房状态影响的 snapshot；若前房最优性本身依赖后房成员，必须联合枚举，不能只在末尾刷新旧选择。
- 完整池与结构化扩池必须覆盖合法候选，不能只依赖工具人白名单。

### 3.4 Rotation 层

- 实际 bind 成员同上同下、上岗 / 休息次数正确。
- 未入选候选不得在轮换层重新强塞。
- producer 休息班不得残留其作用；每班中枢和生产设施满编且无重复干员。
- 暖机稳定只约束实际连续上岗的房间，不构成最低进编班数。

### 3.5 CLI 与导出层

- 至少运行一次用户真实 `plan` 或 `layout team-rotation` 入口。
- profile JSON 和 MAA JSON 使用任务专属路径写入 `out/`。
- 对 stdout / MAA 断言设施类型、实际成员、工作 / 休息状态和核心字段；profile 只断言账号分析与 rotation 指标，不要求它包含最终 MAA 那次完整房间 assignment。
- `plan`、legacy serve 和 `plan.compute` 都通过 `commands/plan_compute.rs` 生成一次用户 rotation；profile、stdout、MAA 和 Worker rotation 摘要必须消费该同一结果。

## 4. 验证留痕（按需）

证据工具是大型重构、solver/search、体系/排班/export、部署和正式审计的可选增强。普通 owner-local 修复、文档调整和一次性诊断可以直接运行必要命令，不要求为了留痕重复执行。需要可复现交付或长期审计时，使用 `scripts/codex/run_evidence.sh` 或兼容包装器，并保留：

- 完整命令和 cwd。
- 输入 layout、operbox、assignment、fixture 和 baseline 路径。
- 开始、结束时间和耗时。
- 完整 stdout + stderr。
- exit code 和明确的 PASS / FAIL 摘要。

真实 `plan` 必须显式指定：

```text
--profile-out out/<task>-profile.json
--maa-out out/<task>-maa.json
```

包装器把日志、status 和聚合 manifest 写入 `target/codex-runs/<task>/`。使用 `--artifact kind=path` 登记 profile、MAA、失败集合报告和其他产物；重复运行不得覆盖旧日志。对于已选择证据工作流的任务，裸跑结果、终端滚屏、Agent 消息、`/tmp` 和 commit hash 不能代替其声明的证据。

选择证据工作流的代码 / 数据任务 manifest 应包含：

- `change_scope`：唯一不变量、根因层、required paths、allowed consumers、proof paths 和 explicitly deferred；
- `scope_expansions` 与 `side_findings`；
- reviewer 最终核对的 invariant、实际 changed paths 和 expansion id；
- runs、artifacts、输入和 exit code。

使用 manifest 的任务完成前运行 `check_task_scope.py` 和 `render_evidence.py`。文档影响按 [文档生命周期](../文档生命周期.md) 由 Agent 直接判断；检查器不根据源码 diff 强制传播文档更新。

证据至少保留到任务交付完成。交付后可按任务目录归档或清理；具有长期审计价值的结论应保存稳定摘要或 artifact，不能假设 `target/` 永远不会被 `cargo clean` 删除。本次工作流不要求迁移或批量删除旧日志。

## 5. 生产检查与失败证据

用户 2026-10-31 裁决不新增、保留或运行源码单元/集成测试；历史源码测试和专用失败集合比较工具删除。Rust 生产终态断言、持久 Box、真实 CLI 回归、数据审计及文档检查继续作为验收入口，不恢复源码测试。

现有 `check_repository_facts.py` 拒绝 Rust 的 `#[test]`、`#[bench]`、`#[cfg(test)]` 和源码测试目录；CI 调用该检查，阻止这些已知形式重新进入仓库。它不声称识别所有条件编译表达式或自定义测试框架。检查当前仓库的 `crates/` 和 `scripts/`，不读取其他工作树。

1. 用 release CLI 运行受影响的真实入口，保存输入、退出码、诊断和完整产物。
2. 涉及 solver、Plan、Team/Shift、rotation、export、跨设施或大规模运行时数据时，运行最终 assignment 的 `--assert-invariants`，并对代表性与 high-owned Box 批量调用；按风险覆盖激活、拒绝和边界反例。
3. 批量结果分开报告不变量失败、求解失败、输入错误和超时；每项失败附具体 Box、profile、命令与诊断。通过只证明这些输入满足已注册不变量，不能证明所有机制、输入或全局最优。
4. 比较前后结果时使用同一输入集合和保证范围，复用 `scripts/assert_rotation_invariants.py --compare-to`；明确变化项与未变化项。失败数量相同不能证明没有新增错误。
5. 既有检查失败仍须核对领域依据与实际行为；删除源码测试不等于修复了它曾报告的差异。历史测试日志保留为调查材料，不再作为当前执行要求或业务真源。

Mechanical Checks 的批量入口显式使用 `--box-source repository`，从 Git 跟踪的既有收集样本构造 representative 与实际持有数不少于 300 的 high-owned 两组；本地默认仍用持久集合。任一请求组为空或样本发现失败时，runner 在构建、求解前以批次错误返回非零，不允许“checked=0”标成验收通过。样本覆盖和输入来源由批量 runner 负责，Rust `--assert-invariants` 仍独立检查最终 assignment；具体命令及计数口径见 [批量入口说明](../../scripts/codex/README.md#批量验收进度)。

## 6. Bake 安全门禁

Bake 是加速载体，不是业务真源。预计算结果只有在候选结构和所有相关上下文与生成模型兼容时才能使用；新写的进度文档不能替代领域裁决或作为机制真源。

### 6.1 当前 schema17 已实现的门禁

- schema、CLI generator 和输入文件 fingerprint 必须一致；输入覆盖 baseline layout、instances、skill table、standalone roster、segments、shortcuts 和 systems。运行时技能表原文及预编译 atom 身份也须兼容。
- catalog 按 `(name, 原始 elite, 有序 buff_ids, tags)` 保存完整技能身份；真实 pool builder 生成练度变体，同名组合互斥。E0/E1 与 full_pool 本身不再必然 miss。冷加载精确核对完整合法组合集合，并按物理 signature 建立 checked dense colex rank 数组；热查询沿 live 子池相对次序直查，保留 live 比较规则及 best/top-K 输出口径。
- room level、capacity、recipe/order 必须有对应 signature；完整 `LayoutContext::search_baseline()`、注入顺序/聚合结果、active_source_buffs 和浮点有限性/位表示须通过门禁。任意布局、动态贸易注入、制造 count-scaled 注入等未覆盖上下文回 live。
- 心情必须有限且大于 0，时长必须有限；Mood/OwnerMoodGap 敏感技能和制造 peer-reset 回 live。制造 `Some(h)` 且候选池含任一 `AddEffRamp` 时整个查询回 live；`None` 的表生成和行 verify 保持 20 小时暖机值。制造 peer-reset 外层绕过已删除，CLI 与核心公开门禁同语义。
- manifest 保存 `combo_table.bin` bytes/hash 和各物理 signature 行数；loader 对同一已打开 File 分块 hash 后 rewind 流式解码并消费，按完整合法 universe bitset 核对缺行、多行、有序身份及贸易链重排。row 保存完整结算响应与必要的 room-local 观察量，查询不改变候选集合或 comparator。
- Auto 缺表/失效时一次使用 Disabled live 生成，完整验证后发布；exe 指纹成功和失败均进程内缓存。独立 generation 目录经验证后由 CURRENT 原子 rename 切换，统一校验、verify、核心/CLI/debug 解析；可用 `ARKNIGHTS_INFRA_BAKED_DIR` 显式隔离生成目录；未设置时沿用 data 根发现顺序。跨进程短发布锁复核目标，输方读取有效胜方或回 live，不发布未验证/limited 表。
- Required 保持只读，公开统一门禁不兼容即严格报 miss，不生成或静默绕过；Auto 的 typed miss 回同语义 live。query_errors 单独记录，原有 rank/row/物化错误仍保留，不能声称所有错误都无条件 fallback。
- 固定 BakeMissKind、冷阶段 calls/errors/nanos 与服务启动同步 warm 已接入；warm 失败继续 live 与 ready。生成物已取消跟踪，历史 schema4 目录已清理，不能手改生成物裁决语义。

当前实现 owner 见 [PERFORMANCE_ENGINEERING.md](PERFORMANCE_ENGINEERING.md)，冻结证据和详细范围见 [性能优化调查与Bake重设计](../ARCHIVE/done/性能优化调查与Bake重设计.md) §8.1。第一轮旧表29,783,417行/3,292查询零差异、2,148跳过以及原选人/MAA/失败差异均保留；后续旧语义消融同样完整表29,783,417行/3,435查询零差异、2,005跳过，152盒及full-e2两入口中实际产出/成员/rotation/MAA零差异，296叶仅同文件输入绝对路径标签，原same5/changed147和逐字相同既有断言失败保留。覆盖内纯性能归因可关闭，不能宣称raw字节相同或推广全部输入/Required+peer-reset。首轮592270b历史证据保留；最终7695096fd新冻结重复原55×3解析及225输入，并新增129输入，总1122full（1050成功、72预期非有限错误）、1050search（798获准精确一致、252既有>=12h负心情扣减政策排除），数学千分与公开f64合同零差异，指定P1/P2静态闭合；不能将252排除写成light/full一致或认证所有实数微段/全技能/全局排班。P1铅踝扣减后槐琥漏读、P2合法极短浮点段拒绝已局部修复并完成新release编译：槐琥分桶在全部EffVar直接贡献后读取（order10→30，五ramp order9保持），极短段保留权重和可表示求值。普通段分段积分，微段保留权重但非线性代表点求值仍受f64舍入约束；新数值和反例已完成，最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。 旧完整表不能替代新语义验收。None20h使用新连续暖机，Some(h)真实时长不round/clamp，发电不改。五例24/24预算确实超限，保持正确gate与canonical既定正常warning fallback，不新增排班policy。

制造 Plan 试排另使用有界的运行时完整条件响应表，支持真实混合练度和动态上下文；它不放宽
上述磁盘 catalog 的兼容门禁。键覆盖、纯单房结果复用和 miss 重算边界见
[PERFORMANCE_ENGINEERING.md](PERFORMANCE_ENGINEERING.md#制造体系试排的缓存取舍2026-10-31)。

### 6.2 真实限制与后续边界

- combo table 已有 bytes/hash 和逐 signature 行数校验；加载期 rank 索引由完整合法集合重建，index checksum 尚未独立持久化，不能写成已完成。
- 完整表与加载期索引已获授权；schema17实际完整表为29,783,417行/3,843,098,117字节，最终WSL已完成新表生成/核心对账及CLI回归。第一轮9.2G/无swap是历史观测；最终cgroup采样和systemd终止摘要不同口径，不拼为单一RSS结论。历史估算不作现行事实，也不重新打开体积裁决；批量 systemd 的小峰值未覆盖实际 solver，不能作为运行时测量。
- 全量行 verify 不能证明所有未覆盖上下文或全部候选子集排序均等价；不兼容查询仍依靠门禁回 live。cache miss 只替代候选生成/求值方式，不缩小候选集，不转用固定 top-K 或旧 pipeline。
- 最终对拍使用原冻结源码及其基线；原 `/tmp/perf-current-20261028` 已因本地 OOM 后环境重启丢失，按确证的 `3201fa7` 加冻结 U1a 四文件重建和采集，持久恢复证据见交接单 §4.1。用户已裁决按正确暖机实现，允许证实由暖机重评分引起的连带选人、排班与聚合变化；纯性能偏差与未归因差异仍须原样报告，不能降低保证等级。

未来联合候选 Bake 的 winner 仍必须在完整临时 assignment 上统一 resolve；DP、Pareto 和安全上界不是首期前提。详见 [DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](../TODO/详情/性能质量/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

## 7. 完成证明

交付前逐条填写：

| 不变量 | 代码保证 | 删除的冲突 | 回归 | 端到端结果 | 日志 | 产物 |
|--------|----------|------------|------|------------|------|------|
| 逐项填写 | 唯一类型 / 字段 / 函数 | 被删除的旧路径 | 真实反例 / 终态诊断名 | 实际房间 / 队伍 / 字段 | 可点击绝对路径 | 可点击绝对路径 |

最终回复还必须单列：

- 根因层和旧模型为什么允许非法状态。
- 新的单一事实源。
- 本轮通过项、既有失败、新增失败和未验证风险。
- 实际运行过的用户入口。
- release Build、终态断言与批量 Box、真实 CLI、性能和 JSON 的验证证据。
- commit hash；未提交时明确写“未提交”。
