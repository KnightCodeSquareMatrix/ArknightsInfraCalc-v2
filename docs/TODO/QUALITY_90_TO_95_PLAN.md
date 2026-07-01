# 90 → 95 质量提升计划：候选架构、机制分析与体系烘焙

> 状态：active
> 启动日期：2026-06-30
> 执行对象：GPT-5.5 / 后续实现 agent
> 目标：在不引入 CP-SAT / MILP / 张量引擎等重型求解器的前提下，把当前约 90 分的推荐质量稳定提升到 95 分。

## 0. 背景与判断

本项目是明日方舟基建效率 / 编排 / 排班引擎。当前核心能力已经可用，主要依赖：

- 手写机制规则；
- shortcut / template；
- baked precomputed combinations；
- runtime fallback exhaustive search；
- 小规模组合穷举，例如制造站 `C(n,3)`；
- 按真实产出分数排序；
- 部分体系认领、跨设施注入和排班约束。

当前问题不是“算不动”，而是：

> 某些房间，尤其制造站，在 shortcut / expert template 较少时，纯局部分数搜索会选出数学上高分但人类玩家觉得奇怪、不稳定或破坏体系语义的组合。

因此本计划不主张替换成大型 OR solver，而是推进轻量混合架构：

```text
规则集定义体系
        ↓
baking 物化高质量候选
        ↓
runtime exhaustive search 补漏
        ↓
统一 Candidate / TeamColumn 进入评估、选择、解释
```

## 1. 非目标

本轮质量提升明确不做：

- 不引入 CP-SAT / MILP / Gurobi / OR-Tools 作为主求解器；
- 不引入 Column Generation / Branch-and-Price 的完整数学规划框架；
- 不引入张量引擎 / GPU / 神经网络排序；
- 不把中文游戏原文自动编译成可执行规则；
- 不一次性重写所有 station；
- 不把真实效率分数和排序偏好混在一起；
- 不牺牲现有 CLI、fixture、回归验证路径。

可以借鉴这些理论中的思想，例如 `candidate columns`、`soft constraints`、`epsilon-optimal preference`，但实现保持项目内轻量自研。

## 2. 总体目标

### 2.1 用户可见目标

- 制造站不再因为 shortcut 少而频繁输出奇怪组合；
- 高效率体系组合，例如“清流 + 温蒂 + 冬时”这类专家认可组合，能优先进入候选；
- 如果普通搜索确实明显更高效，仍允许普通组合胜出；
- 输出能解释为什么选体系组合、为什么没选最高裸分组合；
- 反馈 case 能沉淀为回归测试，避免同类退化。

### 2.2 工程目标

- 建立统一 `TeamCandidate / TeamColumn` 中间层；
- 拆分 `Generate → Evaluate → Select → Explain`；
- 将 `data/MECHANICS_REGISTRY.csv` 用于机制覆盖率审计、体系发现、风险标注和测试生成；
- 将 baking 从“缓存搜索结果”升级为“materialized candidate view”；
- 建立轻量全局候选选择器的演进路径；
- 增加 decision trace，降低调试成本。

## 3. 问题规模说明

请后续实现 agent 不要误判成本项目为大型 workforce scheduling：

- 房间数量少；
- 每个房间小团队，通常 2～3 人；
- 制造站典型规模是从有限候选中选 3 人；
- `C(n,3)` runtime 穷举通常可接受；
- 主要复杂度来自领域规则、体系联动、跨房间影响、配方差异和例外情况；
- 项目更需要稳定、可解释、符合专家直觉的 near-optimal 输出，而不是数学证明的全局最优。

## 4. Phase A：机制注册表分析管线

输入：`data/MECHANICS_REGISTRY.csv`。

该 CSV 是解包技能索引，包含：

```text
序号, 技能名, 工作设施, 产物限定, 干员, 需求精英, 效率值, 游戏原文
```

已知规模约 727 条，其中制造站、贸易站、控制中枢、发电站、宿舍等设施均有覆盖。

### 4.1 目标

建立只读分析管线，输出以下派生表 / 报告：

1. 机制覆盖率审计；
2. 机制标签 taxonomy；
3. 跨设施影响图；
4. 体系候选发现；
5. 高风险技能清单；
6. 回归测试建议清单。

### 4.2 机制标签 taxonomy

至少识别以下标签：

```text
flat_bonus
recipe_specific_bonus
faction_count_bonus
pair_synergy
same_room_synergy
cross_room_synergy
global_bonus
mood_recovery
mood_consumption
capacity_bonus
cap_rule
max_of_same_effect
conditional_if
threshold
negative_effect
special_stacking
resource_token
```

标签可以先基于关键词 / 正则 / 人工映射，但只用于召回和排队，不作为语义判断依据。体系 JSON、回归 seed、runtime rule 必须由 agent / 人工逐条阅读原文、反馈材料与现有代码后生成，并附证据、解释和不确定项。

### 4.3 高风险关键词

优先关注包含以下词的技能：

```text
当与
每个
如果
基建内
额外
上限
同种效果取最高
特殊
制造站
贸易站
发电站
控制中枢
热情值
人间烟火
感知信息
```

这些通常代表跨设施、条件、上限、特殊叠加或体系 token。

### 4.4 输出建议

新增或生成以下报告文件之一：

```text
docs/INTERNAL/MECHANICS_REGISTRY_AUDIT.md
或 target/generated/mechanics_registry_audit.md
```

若报告是手写长期文档，放 `docs/INTERNAL/`；若是脚本输出，放 `target/generated/` 并在文档中说明生成方式。

### 4.5 禁止事项

- 不要把中文原文直接自动转成可执行规则；
- 分析脚本只生成候选索引 / 召回清单 / 审计队列，不能决定 JSON 内容；
- 体系 JSON、反馈回归 seed、runtime rule 必须来自双人或多 agent 人工复核后的交叉一致事实；
- 只整合一致、可追溯、可辩护的部分；分歧必须保留为 `open_questions` 或 `uncertainties`；
- 不要因为分析脚本发现某技能“疑似未覆盖”就直接改公式。

## 5. Phase B：统一 TeamCandidate / TeamColumn 模型

### 5.1 目标

将制造站、贸易站、shortcut、baked combo、动态搜索和体系候选统一表达为候选列。

建议模型：

```rust
struct TeamCandidate {
    station_kind: StationKind,
    room_id: Option<RoomId>,
    recipe: Option<RecipeKind>,
    operators: Vec<OperatorId>,

    source: CandidateSource,
    system_tags: Vec<SystemTag>,

    raw_score: Score,
    decision_score: Score,

    hard_constraints: Vec<ConstraintTag>,
    soft_preferences: Vec<PreferenceTag>,

    explanation: CandidateExplanation,
}
```

候选来源：

```rust
enum CandidateSource {
    SystemBaked,
    GenericBaked,
    Shortcut,
    DynamicSearch,
    Fallback,
    ManualRule,
}
```

### 5.2 分层原则

严格拆分：

```text
Generate → Evaluate → Select → Explain
```

- Generate：只产生候选，不决定谁赢；
- Evaluate：只计算真实效率 / 真实效果，产出 `raw_score`；
- Select：处理去重、冲突、体系偏好、near-optimal tolerance；
- Explain：解释选择原因和替代方案。

### 5.3 分数原则

必须分离：

```text
raw_score      = 真实效率，用于展示、回归、可信解释
decision_score = raw_score + soft preferences，用于排序 / 选择
```

禁止把体系偏好直接写入真实效率。

### 5.4 当前 v0 状态

已引入 shadow `TeamCandidate` / `TeamColumn` 模型，并提供从当前 manufacture / trade search hit 与 manufacture system trace 的 adapter。debug JSON 已暴露从 manufacture trace 投影出的 `TeamCandidate` payload；`system-baked` trace 会投影为 `CandidateSource::SystemBaked`，但该模型暂不参与默认选择，`decision_score` 暂等于 `raw_score`，不触发 near-optimal preference。

## 6. Phase C：制造站体系烘焙候选注入

这是当前最高优先级的业务收益点。

### 6.1 背景

制造站当前 shortcut 较少，更依赖：

```text
候选池筛选 → C(n,3) 穷举 → prod_total 排序
```

这会在候选不足或体系未建模时产生奇怪组合。

### 6.2 目标流程

```text
System-baked manufacture candidates
        +
Generic baked manufacture candidates
        +
Dynamic C(n,3) fallback candidates
        ↓
compatibility check
        ↓
dedup
        ↓
evaluate raw_score
        ↓
near-optimal system preference
        ↓
select / explain
```

### 6.3 Near-optimal preference

推荐规则：

```rust
let best_raw = candidates.iter().map(|c| c.raw_score).max();

for c in candidates {
    c.decision_score = c.raw_score;

    if c.source == CandidateSource::SystemBaked
        && c.raw_score >= best_raw - SYSTEM_TOLERANCE
    {
        c.decision_score += SYSTEM_PRIORITY_BONUS;
    }
}
```

含义：

- 体系组合只比裸分最优低一点点时，优先体系；
- 体系组合明显低很多时，普通最优仍然胜出。

初始建议：

```text
SYSTEM_TOLERANCE: 0.01 ~ 0.03，根据制造站回归调参
SYSTEM_PRIORITY_BONUS: 小于或等于 tolerance
```

### 6.4 体系烘焙候选内容

制造站 system baked row 建议包含：

```text
candidate_id
station_kind = manufacture
recipe
operators
raw_score
source = SystemBaked
system_tags
system_priority
completeness
fingerprint
compatibility_flags
explanation_stub
```

### 6.5 验收 case

优先覆盖当前反馈：

```text
feedback/2026-06-29/010315-推荐调整成清流温蒂冬时-挂钩发电承曦格雷伊-130/operbox.json
```

目标：

- 制造站候选能看到“清流 + 温蒂 + 冬时”类体系组合；
- 发电站挂钩候选能支持“承曦格雷伊”类跨站收益解释；
- 如果最终没选体系，trace 必须说明分差或冲突原因。

### 6.6 当前 pilot 状态

已落地一个局部 Phase C pilot：在制造站公孙自动化金线补位路径中，将“清流 + 温蒂 + 冬时”物化为首个 `system-baked` manufacture row 草案，并把承曦格雷伊记录为 `linked_producer`。当前低练度反馈 case 下，该候选只作为 trace-only / rejected candidate 可见，可给出 `tier_gate_not_met`、`required_buff_missing`、`linked_producer_not_satisfied`、`raw_score_below_selected` 等稳定拒绝原因。

最新修正已覆盖 `operator_used` 场景：当候选干员已被其他制造站占用时，trace 会以 `operator_used` 拒绝，并通过 `assign_manufacture_lines(..., Some(&mut trace_sink))` 集成测试验证 hook wiring。

该 pilot 已支持 opt-in debug JSON 输出，但不接入 CLI 默认输出，不改变最终推荐选择；也不修改 `manufacture/solver.rs`、普通制造搜索池、`search/manufacture.rs` 排序逻辑或 `data/base_systems.json`。后续进入真正候选选择前，仍需设计 near-optimal tolerance、decision score 与更完整的用户可见 trace 输出。

## 7. Phase D：Baking 作为 materialized candidate view

### 7.1 目标

将 baking 从“缓存搜索结果”升级为“物化高质量候选”。

统一 baked row 概念：

```text
TeamCandidateRow
```

来源可以是：

- generic exhaustive baked；
- system baked；
- shortcut baked；
- manual high-confidence template。

### 7.2 要求

- baked row 必须带 fingerprint；
- `base_systems.json`、技能表、干员 catalog、配方相关数据变化时必须触发失效；
- runtime 必须重新做 roster / occupation / recipe / room compatibility check；
- baked row 不能绕过真实求值和解释层。

## 8. Phase E：轻量全局候选选择器

### 8.1 目标

解决单房间局部最优导致的全局不自然问题。

建议先做实验性 selector：

```text
每个房间生成 top K candidates
        ↓
选择一组互不冲突的 candidates
```

约束：

```text
one candidate per room
no duplicate operators
respect hard system constraints
respect occupied / unavailable operators
```

目标函数：

```text
maximize sum(raw_score)
       + system preference bonus
       - fragmentation penalty
       - opportunity cost penalty
```

实现可以是：

- DFS；
- branch-and-bound；
- beam search；
- 当前代码内自研小规模 selector。

不需要 CP-SAT。

### 8.2 推进方式

不要第一步就替换主路径。先：

1. 在 debug / experimental flag 下生成 top K；
2. 与现有结果对比；
3. 输出差异 trace；
4. 只在制造站 / 贸易站争人 case 中试用；
5. 回归稳定后再考虑进入默认路径。

## 9. Phase F：SystemRule registry 正规化

### 9.1 目标

把体系从散落在 shortcut、claim、integrity、bake、search 中的特殊逻辑，提升为一等公民。

建议结构：

```text
system_id
name
station_scope
required operators
optional operators
alternative groups
recipe constraints
priority
exclusive_group
candidate generation capability
claim behavior
completeness score
explanation template
```

### 9.2 体系规则参与面

体系规则应参与：

- candidate generation；
- baking；
- runtime compatibility checks；
- selection preference；
- decision trace；
- explain output；
- regression tests。

## 10. Phase G：Decision trace 与解释层

### 10.1 目标

每次选择都能输出机器可读 trace，降低调试成本。

示例：

```json
{
  "room": "manufacture_1",
  "selected": ["Purestream", "Weedy", "Windflit"],
  "selected_source": "SystemBaked",
  "raw_score": 1.315,
  "decision_score": 1.325,
  "best_raw_alternative": {
    "operators": ["A", "B", "C"],
    "raw_score": 1.322,
    "source": "DynamicSearch"
  },
  "reason": "system candidate within tolerance",
  "rejected": [
    {
      "operators": ["A", "B", "C"],
      "reason": "lower decision score after system preference"
    }
  ]
}
```

### 10.2 要求

当前已增加最小 debug-only 输出：`infra-cli layout test --debug-manufacture-trace [path]` 会在生成新 assignment 时额外收集制造站 `manual-system-candidate` trace，并输出 `manufacture_trace_debug_v0` JSON；默认推荐路径和普通 `layout test` 输出不变。该输出目前覆盖 Phase C pilot 的候选来源 / 拒绝原因 / linked producer，并额外暴露 shadow `TeamCandidate` 投影，但不代表完整解释层完成。

trace 至少能解释：

- 候选来源；
- 原始效率；
- 决策分；
- 是否触发体系偏好；
- 是否因干员冲突被拒；
- 是否因 recipe / room incompatible 被拒；
- 普通裸分最高方案是什么；
- 选择体系方案时的 regret / 分差。

## 11. Phase H：反馈驱动回归测试集

### 11.1 目标

将 `feedback/` 中的真实用户反馈转为长期测试资产。

测试不要求所有结果完全等于人工期望，而是检查：

- 是否选到专家体系；
- 如果没选，是否分差明显或冲突合理；
- 是否没有出现已知 forbidden strange pattern；
- 是否解释了关键取舍；
- 是否没有破坏其他房间。

### 11.2 建议 case schema

```json
{
  "name": "purestream-weedy-windflit-manufacture",
  "input": {
    "layout": "...",
    "operbox": "..."
  },
  "preferred_patterns": [
    {
      "station": "manufacture",
      "operators": ["Purestream", "Weedy", "Windflit"],
      "max_regret": 0.02
    }
  ],
  "forbidden_patterns": [],
  "explanation_expectations": [
    "system candidate within tolerance"
  ]
}
```

## 12. 推荐执行顺序

### Milestone 1：只读分析与文档

- [x] 机制注册表标签 taxonomy 草案；
- [x] 制造站相关机制筛选报告；
- [x] 跨设施影响制造站清单；
- [x] 高风险机制清单；
- [x] 当前反馈 case 梳理。

产物：

- `scripts/audit_mechanics_registry.py`
- `docs/INTERNAL/MECHANICS_REGISTRY_AUDIT.md`

### Milestone 2：制造站候选注入 pilot

- [x] trace-only `manual-system-candidate` pilot：覆盖“清流 + 温蒂 + 冬时”与 linked producer “承曦格雷伊”，并能解释低练度 / 占用等拒绝原因；
- [x] 定义最小 `CandidateSource` / candidate metadata；
- [x] system baked manufacture row 草案（当前仅自动化金线 pilot，trace-only，不参与默认选择）；
- [ ] runtime 合并 system baked + generic baked + dynamic search；
- [ ] raw_score / decision_score 分离；
- [ ] near-optimal preference；
- [ ] 当前反馈 case 回归。

### Milestone 3：trace 与解释

- [x] 最小 CLI debug JSON 输出：`layout test --debug-manufacture-trace [path]` 输出 Phase C pilot trace；
- [x] debug JSON exposes projected `TeamCandidate` payload for manufacture trace pilot；
- [ ] 候选来源 trace；
- [ ] best raw alternative trace；
- [ ] rejected reason trace；
- [ ] 完整用户可见 CLI debug 输出或 JSON 输出；
- [ ] 文档更新。

### Milestone 4：统一 candidate 架构推广

- [ ] 抽出通用 `TeamCandidate`；
- [ ] trade / manufacture 共用部分 selection 逻辑；
- [ ] baking row 标准化；
- [ ] SystemRule registry 与 candidate generator 对接。

### Milestone 5：轻量全局 selector 实验

- [ ] top K candidate generation；
- [ ] no duplicate operator selector；
- [ ] conflict / opportunity cost trace；
- [ ] 与现有主路径对比；
- [ ] 决定是否进入默认路径。

## 13. 对 GPT-5.5 的执行提示

开始实现前请先读：

1. `AGENTS.md`
2. `docs/INDEX.md`
3. `docs/MANUFACTURE_STATUS.md`
4. `docs/BASE_ASSIGNMENT.md`
5. `docs/ORCHESTRATION_LAYER.md`
6. `docs/INTERNAL/CROSS_FACILITY.md`
7. `docs/SCORING_MODEL.md`
8. 本文档

然后只读定位以下代码入口：

```text
crates/infra-core/src/search/manufacture.rs
crates/infra-core/src/pool/manufacture.rs
crates/infra-core/src/manufacture/solver.rs
crates/infra-core/src/manufacture/interpreter.rs
crates/infra-core/src/bake.rs
crates/infra-core/src/layout/system.rs
crates/infra-core/src/layout/assign.rs
data/base_systems.json
data/MECHANICS_REGISTRY.csv
```

执行原则：

- 先报告当前调用链与最小改动点；
- 先做制造站 pilot，不要一口气改全局；
- 新增偏好必须可配置或命名明确；
- 所有真实效率展示继续使用 `raw_score`；
- 任何 decision score 调整必须能在 trace 中解释；
- 每个 milestone 都要有回归命令；
- 不要提交已有用户改动，只提交本轮自己改的文件。

## 14. 验收标准

本计划完成时，应满足：

- [ ] 制造站体系组合能作为候选被看见；
- [ ] 体系组合接近最优时能优先输出；
- [ ] 明显低效体系不会压过普通最优；
- [ ] 真实效率和决策分离；
- [ ] 至少一个反馈 case 进入回归；
- [ ] 机制注册表能生成覆盖 / 风险 / 体系候选报告；
- [ ] decision trace 能解释关键选择；
- [ ] 文档说明当前 90 → 95 质量提升方向；
- [ ] 不引入重型求解器依赖。
