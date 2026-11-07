# Agent 工作流与求解器开发准则交接

> 文档角色：archive
> 生命周期状态：superseded
> 替代项：docs/文档生命周期.md；AGENTS.md；.agents/skills/
> 历史原因：Phase A 已实施，剩余工作流建议由本次生命周期重建和 current Skills 接管
> 快照日期：2026-07-18
> 摘要：保存 Agent 路由简化与求解器准则的历史交接

> 历史原状态：Phase A 已实施；Phase B–D 仍为后续建议
> 更新日期：2026-07-15
> 本文来源：用户与 Agent 的项目讨论、近期 commit / session 复盘，以及一手软件工程与组合优化资料研究
> 本文负责：记录用户意图、项目现状、任务分类、Agent 协作方式、Phase A 实施依据和规则降维求解器的后续开发准则
> 本文不负责：裁决明日方舟业务规则；当前运行时工作流以根 `AGENTS.md`、项目 Skills 和 current 文档为准
> 后续用途：追溯 Phase A 设计依据，并规划尚未实施的 solver assurance 与网站能力

## 1. 为什么重写这份 handoff

旧版 handoff 把项目描述为“正常维护期”，并计划把根 `AGENTS.md` 精简为维护路由器。经过进一步讨论，这个前提不准确。

项目当前同时存在：

- 逻辑边界已经正确、只需局部修改的小 bug；
- 早期实现没有完整遵照领域文档，需要删除补丁路径并恢复单一责任边界的一致性重构；
- 为未来网站继续增加的真实功能；
- 由反复现实摩擦触发的代码质量建设。

旧工作流为了防止历史上的补丁式修复，不断强化“必须找根因、检查完整生命周期、删除冲突路径”。这些规则保护了体系正确性，但也产生了过补偿：AI 容易把局部问题自动升级成大重构。

因此本文取代旧版 handoff 的核心前提：

```text
项目不是默认维护期，也不是默认重构期。
每个任务必须先判断它实际属于哪种变化，再选择工作流和证明义务。
```

改动大小不是目标。小改和大改都可以正确；关键是改动是否停在当前不变量真正需要的责任边界。

## 2. 用户意图

### 2.1 产品定位

本项目是用户与公孙长乐合作开发的明日方舟基建求解器。未来它会作为“基建一站式解决所有问题”网站的核心求解引擎，而不是一个已经定型、只等待维护的 CLI 工具。

当前已经实现的主要排班形态是定时换班下的三次轮班。未来真实需求至少包括：

- 二次轮班的低操作成本 / “摆烂”方案；
- 因菲亚梅塔心情交换、深海体系或其他特殊状态而拆出的四次轮班；
- 自动轮换入口；
- 后续独立设计的 Mower 动态换班模式；
- 网站需要的稳定求解接口、解释信息和可执行产物。

不同换班模式共享干员池、游戏机制、单设施计算、组合合法性和心情基础能力，但不能共享未经证明相同的调度假设。当前评审边界见 [排班改版逻辑设计](../plans/排班改版逻辑设计_公孙长乐评审.md)。

### 2.2 排班术语

后续文档和实现必须区分：

- `Team A / Team B / Team C`：人员轮换分组；
- `Shift 1 / Shift 2 / Shift 3 / Shift 4`：定时执行的完整基建状态；
- 每个 Shift 中的 `BaseAssignment`：该时间点所有房间的最终编制。

当前基础三次轮班可描述为：

```text
Shift 1 = Team A + Team B
Shift 2 = Team B + Team C
Shift 3 = Team C + Team A
```

`Shift 4` 不等于出现 `Team D`。四次轮班可能只是菲亚梅塔或特殊体系把已有排班状态拆得更细。菲亚梅塔是具名的心情交换动作，会让高价值主力多上一班或持续上班；她不是普通宿管，也不是第四套替补队。具体规则见 [Fiammetta.md](../../排班/Fiammetta.md)。

### 2.3 领域知识与工程责任

合作边界为：

- 公孙长乐负责基建知识、组合关系、优先级和排班策略的领域裁决；
- 用户负责把领域知识转化为项目文档、求解器结构、网站和 Agent 工作流；
- AI 负责访谈提取、逻辑审查、实现建模、反例推理、代码实施和验证，但不能替领域专家裁决未知业务语义。

建议的知识闭环为：

```text
公孙长乐领域访谈
→ AI 提取评审稿
→ 文档内部与跨文档一致性审查
→ 问题清单交回公孙长乐 / 用户裁决
→ 标记为 canonical 领域文档
→ 设计唯一责任边界
→ 实现、反例和真实入口验证
```

AI 生成的访谈稿不是自动正确的 canonical 文档。最近已经出现文档前后不一致，因此新文档收到后必须先做逻辑审查；两个当前文档冲突时不能从代码、fixture 或 top hit 猜测答案。

### 2.4 用户希望的 AI 协作方式

用户不希望自己先把工作拆成字段、函数和修改步骤，再让 AI 机械施工。更理想的协作是：

1. 用户提供目标、历史背景、领域判断和重要反例；
2. AI 建立项目模型，区分事实、用户推测、Agent 推断和关键未知项；
3. AI 主动追问会改变语义或架构的少量问题；
4. AI 对用户思路做推演，提出反例、替代模型和可行方案；
5. 双方在讨论中收敛目标和责任边界；
6. 收敛后一次性修改相互关联的工作流文件，避免边聊边把临时观点固化成硬规则。

提示词应提供项目目标、判断原则和停止条件，而不是用大量固定动作替代 AI 的判断能力。

### 2.5 用户对“通用”的要求

用户追求通用实现，不希望为了单一 operbox、room id、Shift 下标、fixture 或当前 top hit 新增补偿分支。但“通用”不表示消灭所有具名领域概念。

应区分：

- **错误实现特判**：下游按名字或样例特征强塞成员，用来补偿上游没有表达的不变量；
- **合法领域专用机制**：菲亚梅塔心情交换、某个真实体系或 shortcut 本身就是领域对象，可以具名存在；
- **通用生命周期**：规则怎样选择、形成 Plan、执行、结算、进入轮换、验证和导出；
- **通用证明接口**：规则为什么能拒绝候选、保持什么语义、如何通过 oracle 或反例验证。

需要通用化的是重复的生命周期、所有权和证明义务，不是把所有明日方舟机制匿名化成万能 DSL。

## 3. 当前项目现状与历史成因

### 3.1 当前不是单一维护阶段

早期使用 Cursor、DeepSeek 等成本较低的方案快速推进，产生了不少“当前样例能过，但没有完整还原文档”的实现。常见形态包括：

- required anchor 没有进入 Plan，只靠 `shift_bind` 或优先级期待自然入选；
- 体系选择和落位由两个边界分别负责；
- 下游 fill 按干员名强塞，用来补偿上游模型缺失；
- shortcut 结算正确，但体系进编没有结构保证；
- 当前 full-E2 top hit 正确，只是候选顺序或房间顺序的巧合；
- 测试把当时的偶然输出固化为正确语义。

因此项目现在每天可能同时发生小 bug 修复、领域一致性修复、较大结构重构和新功能设计。根文件不能继续把“维护期”写成项目生命周期的总判断。

### 3.2 最近 commit 说明了什么

commit 只能证明代码变化，不能单独证明改动是否获得授权或为什么这样设计。近期 session 复盘补充了以下背景：

#### `16a7d67`：正确的小改

红松林数据错误地把普通赤金队友写成非 optional `pick_one`。领域文档已经明确只有砾是该房 anchor，其余队友应交给制造搜索。修复只删除两个错误 `pick_one` 并增加缺队友回归。

这个例子说明：正确责任边界已经存在时，小改不仅允许，而且是最健康的修复。

#### `75692aa` → `708ff13`：决策从 export 回到 schedule

早期菲亚梅塔目标选择发生在 `export/maa.rs`，根据当前 assignment 和固定优先级直接生成字段。后续实现把决策移到 `schedule/`：选择目标的休息 Shift、让主力回到原房、换下替补并重新评分，export 只序列化已确定动作。

这个例子说明：领域机制可以具名，但决策必须位于拥有完整上下文的责任层。

#### `f0e70f5` → `a01fa89`：eligible 与 required 混淆

一度在通用 trade fill 中加入 `search_anchors`，强制尚未使用的候选进入贸易搜索；随后删除，并明确 `Search` slot 表示合法候选而非 required anchor。

这个例子说明：上游概念没有区分 eligibility 和 admission 时，下游强塞很容易形成补丁。

#### `b07f8c4`：经过授权的声明式编排重构

该 commit 大幅收敛专用 evaluator，建立 `Rule → Candidate → AssignmentPlan → Execute` 的通用规则编译路径。session 记录显示：

1. 用户先提出不希望“一体系一个函数”；
2. AI 提出按 roles、relations、gates、alternatives 建模；
3. 用户要求继续优化，避免任意 Predicate AST 和过重 DSL；
4. AI 在实施前列出会改变业务或架构的确认项；
5. 用户逐项回答并明确授权按计划重构。

所以这是独立的质量 / 架构任务，不是 AI 从一个小 bug 擅自扩张。该背景若只留在 session 中，后续 Agent 仅看 diff 很难恢复设计动机。

### 3.3 session 不是长期真源

历史 session 可以用于恢复授权、争议和设计推理，但文件巨大、噪声多，也不保证长期可用。稳定决策应沉淀为：

- canonical 领域文档：回答业务规则是什么；
- ADR / 架构说明：回答为什么选择该责任边界；
- task handoff / manifest：回答某次工作为什么扩大或停止；
- commit：记录实际实现变化；
- evidence：证明命令、输入和输出。

不能要求未来 Agent 通过通读 session 才能理解核心架构。

### 3.4 当前工作流的失真

先前讨论已经得到“维护、功能、代码质量三条通道”的结论，也已经分析过 AI 为什么会“从插座修到发电站”：项目规则擅长阻止修得太浅，却缺少同等强度的停止条件。

但后来压缩根 `AGENTS.md` 与 router handoff 时，主要保留了：

- 正常维护期；
- 最小责任边界；
- 禁止顺手重构；
- 历史 TODO 默认冻结。

而丢失了：

- 项目仍在持续增加真实能力；
- 文档一致性问题有时需要较大重构；
- 代码质量任务可以由重复现实摩擦正式触发；
- 小改和大改都必须由诊断决定，而非提示词预设。

本次工作流修改首先要修复这项信息压缩失真。

## 4. 软件工程一手资料结论

### 4.1 行业没有“永远小改”或“永远修到最上游”的共识

Martin Fowler 的 [Workflows of Refactoring](https://martinfowler.com/articles/workflowsOfRefactoring/) 区分重构与行为改变的“两顶帽子”，同时讨论 preparatory、opportunistic、planned 和 long-term refactoring。核心不是禁止当前任务做 preparatory refactor，而是每一步清楚自己是在保持行为还是改变行为。

[Google Engineering Practices：Small CLs](https://google.github.io/eng-practices/review/developer/small-cls.html) 把小变更定义为一个 self-contained change，通常建议把重构与 feature / bug fix 分开，但允许局部 cleanup 与当前变化同行。这里的“小”不是机械行数，而是单一目标、可独立审阅和验证。

[Microsoft Engineering Fundamentals：Pull Requests](https://microsoft.github.io/code-with-engineering-playbook/code-reviews/pull-requests/) 同样强调 focused、small、单一目标、包含测试且持续可构建。

因此本项目应采用：

```text
一个任务 / 写入单元只承担一个主要意图；
必要的 preparatory refactor 可以属于当前任务；
独立的架构改善不能借 bug 自动并入。
```

### 4.2 根因分析要按比例进行

[Google SRE Postmortem Culture](https://sre.google/sre-book/postmortem-culture/) 强调 contributing causes 和防复发 action，也明确分析有成本，需要触发阈值。复杂系统通常不存在一个可以无限向上追溯的“终极根因”。

本项目要寻找的是：

> 最早允许当前错误类别出现、且能够通过一个可行动责任边界阻止复发的位置。

不是从每个局部 bug 推导到全仓库重构。

### 4.3 技术债按经济价值治理

Fowler 的 [Technical Debt](https://martinfowler.com/bliki/TechnicalDebt.html) 强调本金与利息：高频变更区的坏结构持续产生利息，值得偿还；稳定且不再变化的旧代码即使不美观，也未必值得重写。

本项目独立质量任务应由现实证据触发，例如：

- 同类错误重复出现；
- 同一规则需要在多个生命周期阶段重复特判；
- 一个模块持续成为不同 bug 的根因；
- 每个新功能都必须修改同一批分散路径；
- 文档、代码和测试反复漂移；
- 性能已影响真实网站或 CLI；
- 当前结构无法说明求解完整性或最优性保证。

“代码不够优雅”本身不足以自动启动大重构。

## 5. 任务分类与 AI 自主边界

### 5.1 四类任务

| 模式 | 进入条件 | 默认动作 | 停止条件 |
|------|----------|----------|----------|
| `local-fix` | canonical 文档清楚；现有模型能表达规则；唯一 owner 已存在；错误局限在 owner 内部 | 直接小修、定向反例、真实入口 | owner 内错误修复，相关回归通过 |
| `conformance-fix` | 文档清楚；模型无法表达不变量，或多个阶段分别补偿同一缺口 | 做必要的 preparatory / structural refactor，删除冲突旧路径，再修语义 | 当前不变量有唯一 owner，必需消费者接入，冲突路径删除 |
| `quality-refactor` | 当前 bug 可独立修复；结构问题由重复现实摩擦证明，涉及多个独立语义或长期成本 | 作为独立任务讨论目标、收益基线、迁移和非目标 | 既定迁移完成、旧路径删除、收益与回归证据齐全 |
| `feature` | 新增可行域、目标、策略、排班模式、接口或用户能力 | 先确认用户场景和领域文档，再做纵向闭环 | 用户场景可用、输出和文档一致、非目标明确 |

另有 `semantic-audit` 状态：canonical 文档缺失或冲突时停止实现，先完成领域裁决。

### 5.2 分类判定问题

编辑前依次回答：

1. 当前目标是恢复既有承诺，还是增加新能力？
2. canonical 领域文档是否清楚且内部一致？
3. 当前代码是否已经有唯一、正确的责任 owner？
4. owner 是否能够直接表达这条不变量？
5. 是否存在其他阶段对同一规则的 fallback、强塞或重复判断？
6. 不做结构调整，是否仍能在正确 owner 内修好并防止同类复发？
7. 新发现是否是当前不变量的必要组成，还是独立质量问题？

不能按 diff 大小反向分类任务。

### 5.3 AI 的自主与等待边界

建议后续工作流按以下方式减少用户微操：

- `local-fix`：AI 自主实施，不要求用户批准字段和函数选择；
- `conformance-fix`：领域语义清楚时，AI 先简述不变量、违规位置、责任边界和删除路径，然后自主实施；
- `quality-refactor`：AI 主动提出，但不借当前 bug 自动实施；讨论收敛后再执行；
- `feature`：AI 主动推理方案，但改变用户能力、目标或排班语义前需确认；
- `semantic-audit`：必须等待公孙长乐 / 用户裁决；
- 局部、可逆实现细节不询问；领域语义、产品目标、不可逆迁移和显著架构取舍才询问。

### 5.4 改动范围与停止规则

每个 changed path 必须满足至少一项：

- 直接建立当前唯一不变量；
- 消费该不变量；
- 删除与该不变量冲突的旧路径；
- 证明该不变量；
- 更新因该行为变化而失真的文档。

对每个文件问：

1. 撤销该文件修改，原 bug 或证明是否会重新失效？
2. 它是在删除冲突，还是只让附近架构更整齐？
3. 新抽象是否有真实重复语义，或者能显著简化证明义务？

当唯一 owner 已建立、必需消费者已接入、冲突路径已删除、相应证明已完成、旁支发现已 deferred 时停止。停止条件不是“附近代码全部干净”。

## 6. 本项目作为规则降维 NP-hard 求解器的特殊准则

### 6.1 为什么比普通业务系统要求更高

普通业务系统主要证明“给定输入得到正确输出”。组合优化求解器还必须证明：

- 当前输出合法；
- 没有因错误规则漏掉更优解；或者
- 无法证明完整时，诚实声明结果只是当前找到的可行解。

本项目的领域规则不是求解器外部的装饰。它们可能删除候选、固定组合、拆分子问题或决定搜索顺序，因此每条规则都可能改变可行空间、目标或最优性保证。

### 6.2 规则保证等级

每条会影响候选或搜索的规则至少归入以下一类：

| 等级 | 含义 | 是否可在 exact 模式删除候选 | 证明义务 |
|------|------|-----------------------------|----------|
| `hard_constraint` | 违反 canonical 领域语义，候选非法 | 可以 | 规则 id、输入事实、违规 witness |
| `dominance_reduction` | 存在不劣的保留解或映射 | 可以 | 可行性保持、完整目标不劣、组合兼容性 |
| `symmetry_reduction` | 只保留等价类 canonical representative | 可以 | 置换保持全部约束与目标 |
| `bound_pruning` | 分支上界不可能超过 incumbent | 可以 | 上界覆盖所有未结算正贡献 |
| `search_heuristic` | 只改变先搜哪里或资源分配 | 不可以 | exact 模式可恢复完整候选 |
| `policy_restriction` | 用户主动限制搜索或改变取舍 | 按 policy 可以 | policy 具名、结果声明限定范围 |
| `approximation` | 为时间 / 内存主动放弃完整性 | 可以 | 降低结果保证、记录停止原因与 fallback |

Lee、Zhong 对 dominance nogood 的研究说明：删除解需要支配关系保证保留解在可行性和目标上不差；多条单独安全的规则联合使用时也必须证明兼容，避免共同删除全部最优解。[IJCAI 2020](https://www.ijcai.org/Proceedings/2020/166)

Chu、Stuckey 的通用 dominance 框架同样把 dominance breaking 视为需要证明的语义变换，而非经验排序。[论文 DOI](https://doi.org/10.1007/978-3-642-33558-7_4)

### 6.3 hard constraint、objective、policy 和 heuristic 必须分层

推荐内部概念顺序：

```text
canonical facts
→ hard constraints
→ objective vector
→ user policy
→ sound reductions
→ search heuristics
→ solver
→ trace / certificate
```

- hard constraints 定义什么合法；
- objective 定义什么更好；
- policy 定义用户愿意接受的取舍；
- sound reductions 保持可行域最优值；
- heuristics 只决定计算资源如何使用；
- trace 解释结果和候选淘汰过程。

不能因为某人通常效率低就从 eligibility 中删除，也不能因为当前 top hit 总是某组合就把它写成 hard System。

### 6.4 支配、对称、上界与分解的证明义务

#### 支配剪枝

对被删除候选 `x`，应能构造保留候选或映射 `y`，证明：

- `x` 的任何合法补全都能映射为 `y` 的合法补全；
- `y` 对完整目标向量不劣；
- Shift 1 峰值、后续 Shift、执行复杂度等所有实际目标均被比较；
- 多条 reduction 联合启用后仍保留至少一个最优代表。

#### 对称性消除

[MiniZinc symmetry breaking](https://docs.minizinc.dev/en/stable/efficient.html) 的安全前提是变换保持约束和目标。本项目中“技能看起来相同”不足以证明干员或房间对称；跨设施作用、心情、Team 绑定和 future Shift 质量都可能破坏对称。

#### 上界剪枝

上界必须覆盖尚未结算的 shortcut、跨设施注入、global resource、轮换绑定和未来班次贡献。漏算正贡献的上界不是保守上界，会错误删除最优分支。

#### 问题分解

单房间局部最优不能自动组合成全基建最优。只要存在跨站资源、`used` 竞争、required anchor、Team 绑定或菲亚梅塔动作，子问题之间就有关联。

Logic-based Benders decomposition 通过有效的 feasibility / optimality inference 连接 master 与 subproblem；仅仅把代码拆成多个 solver 不能证明分解正确。[Hooker、Ottosson](https://doi.org/10.1007/s10107-003-0375-9)

本项目每个分解边界应声明：

- 固定了哪些上下文；
- 通过接口传递哪些跨域事实；
- 哪些贡献尚未结算；
- 子问题是否 exact；
- 后续是否联合比较；
- 如果只是贪心，最终如何降低保证声明。

### 6.5 最优性状态必须诚实

[OR-Tools CP-SAT](https://developers.google.com/optimization/cp/cp_solver#cp-sat_return_values) 明确区分 `OPTIMAL`、`FEASIBLE`、`INFEASIBLE` 和 `UNKNOWN`。找到一个高质量方案不等于证明最优，超时未找到也不等于无解。

本项目未来可以考虑具名状态：

- `EXACT_OPTIMAL`：对当前 canonical 模型和目标证明最优；
- `POLICY_OPTIMAL`：只对用户选择的受限 policy 空间证明最优；
- `BEST_FOUND`：当前找到的最好可行方案，未证明最优；
- `UNKNOWN` / `TIME_LIMIT`：搜索未完成；
- `INFEASIBLE`：已经证明 canonical 模型无解；
- `NO_CANDIDATE_UNDER_POLICY`：当前候选策略未找到，不能冒充无解。

“最优”永远只表示对当前已建模机制、数据版本、目标和 policy 最优，不表示对真实游戏中尚未建模的机制最优。

### 6.6 峰值优先应表达为词典序目标

当前定时换班已确认峰值优先。目标更接近：

```text
Shift 1 峰值主力
> 后续 Shift 的周期产出
> 执行复杂度
> 稳定 tie-break
```

不要用未经证明的匿名加权和近似。除非高层权重严格大于所有低层目标可能贡献的总和，否则加权可能悄悄牺牲 Shift 1 峰值。优先采用显式 tuple comparator 或分阶段优化。

### 6.7 独立可行性 checker

任何最终求解结果都应能由一个不复用 search shortcut、priority 和 pruning 的 checker 重新验证：

- 人员唯一性；
- 房间容量和配方；
- required anchor；
- 同房、跨站、在基建内作用域；
- 互斥和 Team / Shift 绑定；
- 分数与资源分量重算；
- schedule 到 export 的状态一致性。

checker 只验证 witness 是否合法，不负责寻找最优解。独立证书检查能降低求解算法与验证器共同犯错的风险；相关思想见 [Certifying Algorithms](https://doi.org/10.1016/j.cosrev.2010.09.009) 与 [Verifying Integer Programming Results](https://arxiv.org/abs/1611.08832)。

### 6.8 小规模穷举 oracle

应保留一个慢、直白、只用于测试的参考实现：

```text
枚举小 operbox / 小 layout 的全部编制
→ 用独立语义解释器过滤
→ 重算完整目标
→ 得到真实最优值
→ 与优化 pipeline 对比
```

oracle 不复用生产降维规则，否则双方会共同犯错。专用 constraint propagator 与通用 table constraint 的变形对照已经被用于发现约束求解器错误，见 [Metamorphic Testing of Constraint Solvers](https://doi.org/10.1007/978-3-319-98334-9_46)。

### 6.9 差分与变形测试

本项目适合维护以下性质：

- 调换 operbox 候选顺序，最优目标不变；
- 改变无业务意义的 room id，语义结果不变；
- 扩大候选池，exact 最大化结果不能变差；
- 添加严格被支配候选，最优值不变；
- 放宽 hard constraint，最大化结果不能变差；
- 收紧 hard constraint，最大化结果不能变好；
- 关闭任一 `safe_reduction`，最优目标不变；
- fresh computation 与 cache / Bake 命中结果一致；
- optimized pipeline 与小规模 oracle 同值；
- plan 经 export / reload 后，编制和目标分量保持一致。

存在多个等价最优解时，默认比较目标和不变量，不钉死具体 lineup；只有定义稳定 tie-break 后才断言具体阵容。

成熟 SMT solver 仍可通过黑盒变异测试发现大量 correctness bug，说明少量人工 fixture 不足以验证求解完整性。[STORM](https://arxiv.org/abs/2004.05934)

### 6.10 性能是语义邻近属性

在本项目中：

- 搜索爆炸或超时会让网站不可用；
- 通过未经证明的候选删除换取速度会改变答案。

因此性能优化必须同时记录正确性保证。benchmark 建议保存：

- commit 与数据 / 规则 hash；
- layout、operbox、objective、policy；
- seed、线程、时限和硬件；
- 候选数、节点数、incumbent、bound；
- 最终求解状态；
- fresh 与 cached 结果差分。

benchmark 集合应按机制、体系、operbox 规模、跨设施关系和排班模式分层，不只保留熟悉 top hit。MIPLIB 的 benchmark 也按实例特征与 solver 表现从大量候选中平衡选取，而非仅选少数熟悉案例。[MIPLIB 2017](https://doi.org/10.1007/s12532-020-00194-3)

### 6.11 Cache 与 Bake 是派生产物

Cache / Bake key 至少应覆盖所有会改变语义的输入：

- 领域数据和规则版本；
- schema / generator 版本；
- objective 与 policy；
- operbox、layout、配方和设施上下文；
- shortcut、cross-facility 和 global resource 数据；
- 会改变求解语义的 feature flags。

cache hit 后仍应做廉价 hard-constraint revalidation；CI 中定期比较 fresh 与 cached 结果。缓存不匹配或损坏必须进入同语义 fallback，不能静默缩小候选集。

### 6.12 Trace 与轻量证明

每次候选淘汰至少应能追溯：

- 所在阶段；
- rule id；
- 领域文档锚点；
- 输入事实；
- 违规 witness、dominator 或 bound；
- 数据和规则版本；
- 保证等级。

不要求立即引入形式证明器，但不能只记录“被体系过滤”。

## 7. 风险分层验证，而不是所有任务同一门禁

当前证据工具负责留痕是正确的，但不同修改不应承担相同的 solver 证明成本。

| 改动类型 | 最低验证 |
|----------|----------|
| 文案、展示、纯 export 字段 | 格式 / 结构 + export fidelity + 对应入口 |
| owner 内局部条件或数据修复 | 最小反例 + 相邻反例 + 真实入口 |
| hard constraint / eligibility | 激活、拒绝、边界反例 + 独立 checker |
| objective / tie-break | 目标分量测试 + 等价最优处理 + 排序反例 |
| safe reduction / pruning | reduction 关闭差分 + 小实例 oracle + 组合规则测试 |
| decomposition / candidate generation | 小实例全局 oracle + 跨域反例 + 候选完整性 |
| cache / Bake / performance | fresh-vs-cache 差分 + fallback + benchmark + 语义状态 |
| Shift / schedule | Team 与 Shift 不变量 + 词典序目标 + 真实 `plan` / MAA |
| 大型质量重构 | old/new differential、迁移完成、旧路径删除、性能与真实入口 |

Full suite、性能和完整 CLI 是否必跑，应由改动风险和用户入口决定；不能让一个纯文档修正或 owner 内三行数据修复承担全局 oracle 成本，也不能让候选剪枝只靠一个 golden snapshot 交付。

## 8. 对根 AGENTS、Skills 和工作流文档的后续要求

### 8.1 根 `AGENTS.md`

根文件应成为：

```text
项目定位 + 任务分类器 + 真源优先级 + 所有任务通用硬约束 + 路由
```

必须首层可见：

- 项目持续发展，不默认处于维护期或重构期；
- `local-fix / conformance-fix / quality-refactor / feature / semantic-audit` 分类；
- 用户裁决和 canonical 领域 Markdown 的优先级；
- 小改允许，大改也允许，改动大小服从责任边界；
- 搜索空间变化需要声明保证等级；
- 工作区所有权、证据留痕和 Git 安全。

不应继续全文复制：

- 具体 CLI 命令；
- 完整四项审计解释；
- 每个体系的详细规则；
- 完成证明表；
- 模块和 bug 路由大表；
- 某次实时工作区状态。

### 8.2 `arknights-maintenance`

该 Skill 不应再代表整个项目生命周期。它可以负责：

- `local-fix`；
- 结果复现和分层定位；
- owner 内最小修复；
- 普通 CLI、数据、solver 和输出问题。

需要明确：小改不是可疑行为；只有 owner 不存在或不正确时才升级。

### 8.3 一致性 / 体系审计 Skill

当前 `arknights-system-audit` 已覆盖体系、跨设施、required anchor、scope 和 rotation。后续应决定：

- 扩展为更广义的 `conformance-fix` 工作流；或者
- 保持体系专用，再为非体系的模型一致性建立独立入口。

不要在讨论未完成前直接新增万能 Skill。

### 8.4 `arknights-evidence`

继续负责统一留痕和事实核对，但验证矩阵需要按改动风险分层。renderer 负责报告“跑了什么”，不能替代：

- 领域语义审查；
- reduction 健全性；
- oracle 差分；
- exact / heuristic 声明。

### 8.5 现有流程文档

后续一次性修改至少需要重新审视：

- `docs/质量与维护/MAINTENANCE_MODE.md`：从项目总状态降为局部修复工作流；
- `docs/质量与维护/SYSTEM_AUDIT_WORKFLOW.md`：保留 strict audit 与领域冲突等待点；
- `docs/质量与维护/QUALITY_AND_AUDIT.md`：加入风险分层和求解保证等级；
- `docs/INDEX.md`：增加任务模式和 solver assurance 路由；
- `docs/PROJECT_MAP.md`：只记录当前实现事实，不写项目永远处于维护期；
- 排班文档：统一 Team / Shift 术语和不同换班模式边界。

## 9. 后续实施建议

### Phase A：修正 Agent 工作流表达（已完成）

1. [x] 以本文为设计基线，重写根 `AGENTS.md`；
2. [x] 调整 maintenance、system audit、evidence 三个 Skill，并新增 feature / quality owner；
3. [x] 同步 `MAINTENANCE_MODE`、`QUALITY_AND_AUDIT`、`INDEX`、`PROJECT_MAP`；
4. [x] 在根路由和文档入口统一 Team / Shift 术语；
5. [x] 用只读 feature / quality 前向任务验证新 Skill 的触发、停止与渐进读取；真实 debug / system 任务继续作为后续运行时观察。

本阶段作为独立 workflow / quality 任务实施，未修改业务代码、数据或领域语义。

### Phase B：建立求解保证词汇

1. 盘点当前 hard constraint、priority、shortcut、top-K、Bake 和 fallback；
2. 给规则标注 `hard / safe reduction / heuristic / policy / approximation`；
3. 审计文档和 CLI 中“最优”“完整”“无解”等表述；
4. 明确当前各入口真实能声明的保证等级。

先形成文档和审计结果，不要求一次性重写所有 schema。

### Phase C：建立 checker 与小规模 oracle

1. 先做独立最终 assignment checker；
2. 再选一个小 layout / 小 operbox 建立穷举 oracle；
3. 为一条真实 reduction 建立开关差分；
4. 扩展 property / metamorphic 测试；
5. 最后接入 Bake、跨设施和完整排班的分层验证。

该阶段属于正式 solver assurance 功能，不应借普通 bug 顺手实施。

### Phase D：面向网站演进

在求解保证和模式边界清楚后，再逐步推进：

- 定时换班的二次 / 三次 / 四次轮班；
- 菲亚梅塔和深海等状态拆分；
- 自动轮换；
- 后续独立的 Mower 动态模式；
- 网站解释信息与结果保证展示。

不同模式复用共同机制和 checker，但不共享未经证明相同的调度器。

## 10. 明确非目标

本文不授权：

- 立即重构现有求解器；
- 立即新增通用 DSL 或万能 System schema；
- 一次性为所有规则补形式证明；
- 把所有历史 heuristic 自动改成 exact；
- 把 Mower 动态换班塞进定时换班求解器；
- 因为未来网站需要而提前实现全部二次 / 四次轮班；
- 修改任何公孙长乐领域规则；
- 把每个小 bug 都升级为 conformance audit；
- 让每个局部修复都运行完整 oracle、性能和全套 CLI；
- 仅因代码不够优雅而自动启动大重构。

## 11. 后续讨论仍需收敛的问题

在一次性修改工作流文件前，还应继续讨论：

1. `conformance-fix` 是否只覆盖体系 / 编排，还是覆盖所有模型与文档一致性问题；
2. 独立 `quality-refactor` 由哪些现实证据触发，以及 AI 是否可以自主启动；
3. 当前 CLI / 网站对“最优”的真实承诺是什么；
4. 哪些现有 candidate filter、top-K、Bake 和 shortcut 是 exact-equivalent，哪些只是 policy / heuristic；
5. checker 应复用哪些纯语义函数，怎样避免与生产 solver 共同犯错；
6. 第一个小规模 oracle 应选单房间、最小 layout 还是 Team / Shift 子问题；
7. 证据矩阵怎样按风险分层，又不削弱高风险体系和搜索改动的完成证明；
8. 哪些 session 中的长期设计决定需要补成 ADR；
9. `quality-refactor` 与 feature 能否在同一个纵向任务中组合，以及何时必须拆开；
10. 二次 / 四次轮班进入实现前，`docs/排班/定时换班.md` 还需要哪些领域裁决。

## 12. 一句话原则

```text
允许小改，也允许大重构；改动大小由当前不变量的真实责任边界决定。
任何会缩小搜索空间的规则，都必须说明它是领域约束、可证明安全的降维，
还是会降低最优性声明的启发式或用户 policy。
```
