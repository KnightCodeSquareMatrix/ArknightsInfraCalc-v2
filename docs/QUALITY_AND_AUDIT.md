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

- 普通 bug 和结果不对：使用项目 Skill `arknights-maintenance`；复现入口不清时再读 [MAINTENANCE_MODE.md](MAINTENANCE_MODE.md) 对应章节。
- 体系 / 编排正式审计：只在 `formal-audit` 模式读取 [SYSTEM_AUDIT_WORKFLOW.md](SYSTEM_AUDIT_WORKFLOW.md)。
- 当前代码与命令事实不清时，定向读取 [PROJECT_MAP.md](PROJECT_MAP.md)；领域文档未知时由 [INDEX.md](INDEX.md) 路由。
- 命令执行和 manifest：使用 [scripts/codex/README.md](../scripts/codex/README.md)。

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

自动循环的任务包必须声明：目标、用户已授权范围、canonical 路径、允许 changed paths、保证等级、required proof obligations、最大轮数和外部动作策略。默认上限是同一 finding 两次自动修复、整个任务四个修复单元；达到任一上限时以 `UNRESOLVED_EVIDENCE_DISPUTE` 暂停并保留 checkpoint，不得无界重试。每轮 final status 只能是：

```text
AUTO_STATUS: CONTINUE | PAUSED_FOR_USER | COMPLETED
PAUSE_REASON: <one enum above, only when paused>
FROZEN_PACKET_SHA256:
EVIDENCE_REFS:
NEXT_IMPLEMENTATION_UNIT:
```

`COMPLETED` 仍要求该风险等级的全部 gate 和独立复审通过；reviewer 的 `APPROVED`、单个命令成功或实现 Agent 的总结均不能单独关闭任务。当前 harness 尚未实现持久 writer lease、自动重试或用户通知 runtime；本节先定义所有 runtime 必须遵守的决策合同，不声称这些能力已落地。

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

使用 manifest 的任务完成前运行 `check_task_scope.py` 和 `render_evidence.py`。文档影响按 [文档生命周期](文档生命周期.md) 由 Agent 直接判断；检查器不根据源码 diff 强制传播文档更新。

证据至少保留到任务交付完成。交付后可按任务目录归档或清理；具有长期审计价值的结论应保存稳定摘要或 artifact，不能假设 `target/` 永远不会被 `cargo clean` 删除。本次工作流不要求迁移或批量删除旧日志。

## 5. Full suite 与失败基线

本仓库当前不能被笼统描述为“全套测试全绿”。历史和当前证据中存在既有失败；具体集合以本轮开始前保存的完整 baseline 日志为准，不在本文硬编码一个容易过时的数量。

Full suite 验收必须：

1. 保存完整失败列表。
2. 从原始 baseline 和当前日志提取测试全名集合。
3. 分别报告 additions、removals 和 unchanged；不能只比较失败数量。
4. 新增失败为 0 才能宣称“没有新增 full-suite 回归”。
5. 修复或删除一个旧失败必须说明其业务依据；旧测试可能本身保护错误语义。

使用 `scripts/codex/compare_test_failures.py` 从完整日志生成 JSON 与 Markdown；新增失败返回非零，日志截断或格式不识别必须硬失败，不能解析成空集合成功。

Baseline 是检测回归的工具，不是允许既有错误永久存在的豁免，也不能用于宣称全套测试通过。

## 6. Bake 安全门禁

Bake 是加速载体，不是业务真源。预计算结果只有在候选结构和所有相关上下文与生成模型兼容时才能使用。

### 6.1 当前 schema v13 已实现的门禁

- schema、CLI generator 和输入文件 fingerprint 一致；输入覆盖 baseline layout、instances、skill table、standalone roster、segments、shortcuts 和 systems。
- runtime pool 中的候选名必须被 catalog 覆盖，且当前快速路径要求兼容的 E2 tier 模型。
- room level、capacity、recipe / order、mood、shift hours 与 baseline context 必须满足各设施 gate。
- 动态 inject、候选投影、`OperatorInBase` / `OperatorInTrade`、跨房 workforce 或 `full_pool` 等未被当前表精确表达的上下文会拒绝 Bake。
- catalog 缺失、schema / generator / input mismatch 时返回实时搜索。
- manifest 记录 `combo_table.bin` bytes/hash 和每个物理 signature 的行数；loader 对同一份 bytes
  完成反序列化、结构校验与消费，不在校验后重新读取另一份快照。
- 完整贸易 row API 会从当前 TradePool 重建每个物理 signature 的全部合法 operator identity
  集合并与 catalog 精确对照；任何缺行、多行或 identity 变化都拒绝 catalog。
- 贸易 row 可显式保存首批 room-local 机制观察量：peer count、Vina 的 Glasgow peer presence、
  Morgan 的 owner-inclusive Glasgow count、推进之王 presence、`snhunt` / `laterano` count；
  这些字段从实际 row 编译，不改变候选集合或 comparator。

当前工作区旧 catalog 与代码 schema 的具体状态见 [PERFORMANCE_ENGINEERING.md](PERFORMANCE_ENGINEERING.md)。当前 loader 已校验 `combo_table.bin` 的 bytes、内容 hash 和每个物理 signature 的行数；非白名单反序列化错误仍可能作为错误返回，而不是无条件 fallback；生成物也尚未采用完整 generation-id 原子切换。因此不能把下面的未来要求写成当前能力。

### 6.2 下一代 catalog 必须补齐

- 在已有 bytes、内容 hash 和逐 signature 行数校验之外，补齐 index checksum。
- 损坏、反序列化失败、未知 signature 与缺行全部安全进入同语义 live 求值。
- 临时 generation 目录、完整校验和原子切换，避免读取新旧文件混合状态。
- cache miss 只替代候选生成 / 求值方式，不能改用更小候选集、固定 top-K 或旧 pipeline。

未来联合候选 Bake 的 winner 仍必须在完整临时 assignment 上统一 resolve；DP、Pareto 和安全上界不是首期前提。详见 [DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

## 7. 完成证明

交付前逐条填写：

| 不变量 | 代码保证 | 删除的冲突 | 回归 | 端到端结果 | 日志 | 产物 |
|--------|----------|------------|------|------------|------|------|
| 逐项填写 | 唯一类型 / 字段 / 函数 | 被删除的旧路径 | 测试名与断言 | 实际房间 / 队伍 / 字段 | 可点击绝对路径 | 可点击绝对路径 |

最终回复还必须单列：

- 根因层和旧模型为什么允许非法状态。
- 新的单一事实源。
- 本轮通过项、既有失败、新增失败和未验证风险。
- 实际运行过的用户入口。
- Build、定向测试、full suite、真实 CLI、性能和 JSON 的验证证据。
- commit hash；未提交时明确写“未提交”。
