# AI 编排与 Harness 讨论记录

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/QUALITY_AND_AUDIT.md
> 摘要：记录 AI 编排与 Harness 的讨论背景、实验事实、当前判断和未决方向

本文记录讨论过程、外部调研结果、当前判断、未决问题和实验方向。它不替代项目领域 canonical，也不授权直接修改业务 solver。
后续围绕本主题形成新的分析、外部调研结论或用户裁决时，主动追加到本文；事实、推测、建议和未决项必须分开记录。

---

## 1. 起点与用户目标

用户首先观察到：在 OpenCode 中使用 GPT 对话时，subagent 经常不能像 Codex 或 Claude 那样主动并行多个 agent。

用户的实际诉求不是修复一个明确 bug，而是改善 AI-native 团队的工作体验和吞吐：

- 独立调查、搜索、审查应默认并行；
- 主 agent 负责拆解、汇总和决策；
- 共享工作区的实现保持单 writer；
- 长任务可以恢复，而不是每次重新解释项目背景；
- 项目事实和团队经验可以被持续维护；
- 任务过程可读、可追溯、可验证；
- 这个小项目可以承担较大胆的编排框架实验。

用户明确表示，团队是 AI-native 团队，在这个项目上进行大型编排实验是可行的；因此不应只按“保守维护、最低改动”来选型。

---

## 2. 当前项目事实

### 2.1 业务编排不是空白设计

项目当前已经有一套确定性的基建业务编排：

```text
Rule → Candidate → Plan → Execute → Fill → Resolve
```

当前 canonical 责任边界包括：

- `Rule / alternative / gate / role / relation` 表达体系和硬约束；
- `AssignmentPlan` 保存已经解析的 placement、anchor、bind、dependency 和 reserve；
- `execute_plan` 只执行已经解析的计划，不重新选型；
- `fill` 负责普通设施候选和局部效率搜索；
- `resolve` 负责全局上下文、cross-facility/global effect 和 per-room 输入；
- L1/L2/L3 负责机制解释、局部求解和实际组合结算；
- schedule/rotation 消费已经形成的 plan 和 resolved facts。

现行 canonical：

- [编排层重构路线图](ORCHESTRATION_LAYER.md)
- [全基建进驻编制](BASE_ASSIGNMENT.md)
- [架构导览](ARCHITECTURE_TOUR.md)
- [跨设施编排层内部地图](INTERNAL/CROSS_FACILITY.md)

### 2.2 项目已有协作硬约束

根 `AGENTS.md` 已规定：

- 只读调查可以并行；
- 共享 workspace 保持一个 writer；
- 并行 writer 必须使用从明确 base SHA 建立的隔离 worktree；
- 领域语义进入 canonical Markdown，架构决策进入 ADR/change；
- Agent memory 只能保存可丢弃的检索提示和协作偏好，不能成为领域真源；
- 修改候选集合、剪枝、分解、目标、top-K、Bake 或 cache 时，必须说明其是 hard constraint、safe reduction、heuristic、policy 或 approximation；
- 验证结论通过统一 evidence 工具留痕；
- Rust 修改需要 `cargo fmt --all`，格式、测试、CLI 结论需要记录验证类别。

这些规则在语义上已经接近成熟 AI-native workflow，但目前主要由 prompt、文档和 agent 遵守，缺少统一的运行时调度、状态、记忆和强制执行层。

### 2.3 当前 OpenCode 配置事实

当前项目 `opencode.json` 只对 `build` agent 配置了 Task 权限：

- `plan-reviewer`
- `terra-explorer`
- `luna-extractor`
- `sol-reviewer`

其他 Task 默认 deny。项目内未发现 `.opencode/` 编排目录；当前 OpenCode AI 编排能力主要来自全局配置、根 `AGENTS.md`、Skills 和模型自行调用 Task。

因此当前缺少：

- 明确的 orchestrator agent；
- wave/fan-out/join 调度；
- 后台任务持久化；
- 统一 task ledger；
- writer/worktree lease；
- 自动 checkpoint 和 context bootstrap；
- memory capture / distill；
- 完成条件 verifier；
- session parent-child 可视化和 trace。

---

## 3. 关于 OpenCode 并行体验的判断

### 3.1 为什么 GPT 在 OpenCode 中不总是并行

OpenCode 原生提供的是 subagent/Task 能力，而不是默认的强制并行调度器。主模型需要自己决定：

- 是否拆分任务；
- 拆分几个；
- 是否在同一轮发出多个 Task；
- 是否等待一个任务后再启动另一个；
- 是否进行结果汇总。

因此 Codex/Claude 中更强的并行体验，部分来自产品级系统提示、agent view、后台 session、worktree 和 workflow，而不只是模型本身更愿意调用 subagent。

### 3.2 并行的适用边界

适合默认并行：

- 多个只读代码调查；
- 多个独立文档/外部资料搜索；
- 多个独立审查角度；
- 不依赖共享可变状态的验证；
- 多个候选方案的独立评估。

应保持串行：

- 依赖前一步结果的决策；
- 修改同一批文件；
- 共享 workspace 的写入；
- 需要统一领域裁决的任务；
- 需要前置 canonical 或用户判断的工作。

讨论中形成的目标策略是：

```text
独立调查并行
→ 结果汇总
→ 单 writer 实现
→ 独立审查并行
→ 修复
→ 验证并行
→ 最终汇总
```

---

## 4. 方案讨论过程

### 4.1 仅修改 OpenCode 主 agent prompt

最初考虑在主 agent prompt 中加入：

```text
对独立的只读调查，在同一轮同时启动多个 subagent；不要等待一个完成后再启动另一个。
共享文件的实现和依赖性决策保持串行。
```

结论：这能提高并行倾向，但不能硬保证。它仍然把调度责任交给模型，无法提供可靠的 join、重试、恢复和状态可见性。

### 4.2 OpenCode plugin / background agent

外部调研发现以下 OpenCode 项目：

- [opencode-background-agents](https://github.com/kdcokenny/opencode-background-agents)：后台 delegation、结果持久化、通知和恢复；当前限制后台 delegation 只能用于只读 subagent，写入仍使用原生 Task。
- [opencode-arise](https://github.com/bluelovers/opencode-arise)：Monarch 主编排器、角色分工和后台任务工具，例如 `arise_background`、`arise_background_output`、`arise_background_status`。
- [CrewBee](https://github.com/CrewBeeLab/CrewBee)：Agent Team asset、角色合同、review flow、completion criteria 和 OpenCode 投影。
- [FlowDeck](https://github.com/DVNghiem/FlowDeck)：固定 pipeline、wave-based parallel execution、checkpoint、验证和治理层。

结论：OpenCode plugin/SDK 是低迁移成本的近期入口，但 plugin 不能自动保证模型调用它；要真正并行，需要一次调用接收结构化 `tasks[]`，由宿主批量启动 child sessions。

### 4.3 “换一整套外部编排框架”

讨论过的候选：

- OpenAI Agents SDK：适合 OpenAI 模型、工具、handoff、guardrail、trace 和独立 agent runtime；不自动解决项目级 workflow 和持久化真源。
- LangGraph：适合显式状态图、checkpoint、条件分支和 workflow 控制；需要引入另一套运行时。
- Mastra：TypeScript 方向和产品化 Web/API 方向较合适；需要验证其对本项目 workflow、memory 和 worker isolation 的适配。
- Temporal：适合跨进程、长时间、人工审批、重试和分布式 worker；对当前本地实验可能过重，但团队性质允许后续正式试验。
- CrewAI / AutoGen：适合快速探索，但当前没有足够理由作为核心生产控制面。

早期判断曾建议“先做轻量 OpenCode plugin，暂不引入重量级框架”。用户说明团队可以在小项目上承担大型实验后，判断调整为：

```text
可以正式建设 AI 编排运行时；不必把实验限制为小插件。
```

但仍保留业务 solver 与 AI workflow 的边界：外部 AI 编排器不替换 Rust 业务编排。

### 4.4 Orchestrator-first 与 OpenCode 的关系

讨论过是否需要舍弃 OpenCode。结论：不需要。

推荐分工：

```text
OpenCode
  ├── TUI/Web 交互
  ├── 人类审批
  ├── 本地文件和 shell 工具
  └── agent worker 执行面

AI Orchestrator
  ├── workflow 状态机
  ├── wave 并行
  ├── memory/context
  ├── retry/resume
  ├── writer 调度
  └── 结果汇总
```

Orchestrator-first 不应隐藏 OpenCode child session。需要保留：

- 主线只显示高信号状态、摘要和决策；
- 每个任务保存 `session_id`；
- 原始 child transcript 可跳转；
- 结果写入结构化 artifact；
- 任务树和 parent-child 关系可观察；
- 失败、等待输入和重试状态可见。

否则会出现“调度增强、可读性下降”的问题。

---

## 5. 记忆问题

### 5.1 用户观察

当前一些事实性问题需要在不同 session 反复说明；用户感觉 Claude 会自觉维护 memory，而 OpenCode/GPT 不会。

### 5.2 记忆层次判断

不能把所有信息放进同一种 memory。建议分层：

```text
Canonical Facts
  docs/*.md、AGENTS.md、ADR
  领域事实、架构边界、用户裁决

Project Context
  项目定位、模块地图、最近决策、活动任务、已知风险

Session State
  当前任务、子任务状态、已完成步骤、待处理结果

Retrieval Hints
  文件入口、关键词、历史调查位置
```

当前仓库规则要求 canonical 文档成为领域真源，因此自动 memory 只能保存偏好、纠正、导航提示、进度和候选记忆，不能自动覆盖 canonical 语义。

### 5.3 Claude memory 的公开事实

Claude Code 官方文档：[How Claude remembers your project](https://code.claude.com/docs/en/memory.md)

其公开设计包含：

- 项目/用户/组织范围的 `CLAUDE.md`；
- path-scoped rules；
- 自动 memory；
- 每个项目的 `MEMORY.md` 索引和 topic 文件；
- subagent 独立 memory；
- `/memory` 和 `/context` 检查；
- hooks 作为真正的强制执行层。

公开文档说明，auto memory 主要记录：

- 用户角色、偏好和纠正；
- 项目进度和无法从代码推导的上下文；
- 外部参考位置。

它不是对所有代码事实的自动百科化存储。必须执行的规则仍应由 hooks、权限或确定性检查完成。

### 5.4 MiMo 的 memory 设计

MiMo Code 的公开设计进一步拆分了：

- `MEMORY.md`：project memory；
- `checkpoint.md`：session checkpoint；
- `notes.md`：主 agent 可追加的临时 scratchpad；
- `tasks/<id>/progress.md`：任务进度；
- SQLite history：完整 session/tool trace；
- checkpoint writer：独立 agent 定期提取状态；
- Dream：去重、压缩、校验过期记忆；
- Distill：把重复流程沉淀为 skills、commands、agents 或 SOP。

参考：[MiMo Code: Scaling Coding Agents to Long-Horizon Tasks](https://mimo.xiaomi.com/blog/mimo-code-long-horizon)

这个设计解决了“主 agent 一边 debug 一边维护记忆”导致两件事都做不好的问题。对本项目最有价值的是 checkpoint writer、分层 memory 和可审阅的文件型状态。

---

## 6. MiMo Code 评估

### 6.1 公开事实

[XiaomiMiMo/MiMo-Code](https://github.com/XiaomiMiMo/MiMo-Code) 是基于 OpenCode 的开源 coding harness，MIT license。公开功能包括：

- `build`、`plan`、`compose` agent；
- SQLite FTS5 project/session memory；
- automatic checkpoint 和 context reconstruction；
- tree-shaped task tracking；
- goal / independent stop-condition verifier；
- subagent lifecycle、cancellation、background execution；
- Compose workflow；
- deterministic JavaScript workflows；
- workflow 中的 `agent()`、`parallel()`、`pipeline()`；
- `deep-research`、`fact-check`、`research-experiment` 等 workflow；
- `/dream` 和 `/distill`；
- 多 provider 和 OpenAI-compatible endpoint。

其 `compose` 大致是：

```text
Brainstorm → Design → Implement → Verify → Review → Report → Merge
```

其 workflow runtime 能把流程从自然语言变成代码，支持并行、pipeline、有限重试、结果落盘和中断恢复。

### 6.2 适配项目的地方

MiMo Code 特别适合验证：

- 长任务 context continuity；
- 自动 checkpoint；
- project/session/task memory；
- workflow code 化；
- independent goal verifier；
- 多 agent 并行与结果汇总；
- Dream/Distill 对团队经验的沉淀。

它比从零手搓完整 harness 更适合作为实验基线。

### 6.3 不应原样接受的地方

MiMo 的 Compose 不能直接替代本项目 workflow。适配后的流程应是：

```text
Bootstrap Context
→ Domain Owner Routing
→ Parallel Read-only Investigation
→ Plan Review
→ Single Writer Implementation
→ Evidence Verification
→ Independent Review
→ Documentation Lifecycle Update
→ Memory Capture
```

必须保留：

- `AGENTS.md` 的 single writer；
- canonical Markdown 真源；
- `arknights-evidence` 统一证据协议；
- `arknights-system-audit` 体系/跨设施/required anchor 审计；
- 不把 memory 当作领域裁决；
- 不让自动 workflow 绕过当前 `build_plan → execute_plan → fill → resolve`；
- 不让 AI workflow 修改 Rust solver 的业务语义边界。

结论：MiMo Code 不是概念稿，已经值得作为主实验平台；但应通过项目级 workflow、skills 和 memory policy 适配，而不是直接把其默认 Compose 当作项目规则。

---

## 7. GPT-5.6 发布后的调研结果

### 7.1 官方发布与 Multi-agent

OpenAI GPT-5.6 发布说明：[GPT-5.6](https://openai.com/index/gpt-5-6/)

公开能力包括：

- GPT-5.6 Sol/Terra/Luna 多档模型；
- `max` 更高 test-time compute；
- `ultra` 默认并行协调多个 agent；
- Responses API Multi-agent beta；
- Programmatic Tool Calling；
- 更强长任务、工具使用和 computer use。

Responses API Multi-agent 文档：[Multi-agent](https://developers.openai.com/api/docs/guides/responses-multi-agent)

其公开约束和建议：

- 默认 `max_concurrent_subagents` 推荐为 3；
- 并行适合独立、边界清晰的工作流；
- 有共享可变状态、强顺序依赖或需要固定确定性图时，不应盲目使用 multi-agent；
- root agent 负责 spawn、message、wait 和最终 synthesis；
- WebSocket 更适合工具密集、长时间 multi-agent workflow；
- Multi-agent beta 有 compaction、reasoning summary、max_tool_calls 等限制。

Programmatic Tool Calling 文档：[Programmatic Tool Calling](https://developers.openai.com/api/docs/guides/tools-programmatic-tool-calling)

其适用边界：

- 适合确定性、可聚合、可去重、可重试的工具调用；
- 可在 hosted JavaScript runtime 中使用并发、循环和条件；
- 工具应返回紧凑、结构化、带 schema 的结果；
- 写入和 approval-sensitive action 默认使用 direct tool calling；
- 最终引用、原生 artifact 和语义判断不应被程序化工具调用吞掉。

### 7.2 Ploy 的生产迁移经验

来源：[Migrating a production AI agent to GPT-5.6](https://ploy.ai/blog/migrating-a-production-ai-agent-to-gpt-5-6)

Ploy 从 Claude Opus 迁移到 GPT-5.6 时发现：

- GPT-5.6 任务完成 wall-clock 大幅降低；
- token 和成本下降；
- 原有 tool-call budget 按旧模型的串行习惯设计，误判了 GPT-5.6 的并行调用；
- GPT-5.6 会为大量 optional tool fields 填 plausible defaults；
- 工具如果把空结果也返回 `success: true`，模型无法分辨读取失败；
- 通过 provider boundary 的 schema transform，把可选值变成 nullable required 后，空读取问题消失；
- OpenAI prompt cache 与 Claude cache 的范围和 key 语义不同；
- workspace-scoped cache key 比 global key 或 conversation key 更适合其流量；
- reasoning replay 需要处理 server-side item 引用丢失，或使用自包含 reasoning content。

这条经验对当前项目非常重要：GPT-5.6 迁移必须先做 harness compatibility sweep，不能直接把“模型能力提升”当作“旧工具接口仍正确”。

### 7.3 公开合作方评价的使用方式

OpenAI 发布文中收录了 Cursor、Qodo、Notion、Cognition、Ramp、Shopify、Cisco、Clio 等合作方评价，主要提到：

- 更少 token 和 tool call；
- 更强长任务坚持度；
- 更好的 research/design 和 code review；
- 更少卡住和更低成本；
- 更好的 custom agents/memory 更新。

这些是合作方或厂商发布材料，不应当作独立 benchmark；但它们与 Ploy 的工程迁移细节、MiMo 的 harness 设计方向相互印证：新模型收益依赖 provider boundary、workflow、memory 和验证机制。

### 7.4 社区反馈的共同点

公开 HN/博客讨论中出现的共性：

- GPT-5.6 更愿意并行和使用工具，但旧 harness 可能把并行当作异常；
- 多 agent 不一定自动提高质量，handoff 和结果汇总会增加成本；
- 任务越长，checkpoint、进度、独立 verifier 越重要；
- 便宜模型适合提取、分类、搜索、状态维护和机械验证；
- 最强模型应保留给架构、复杂 debug、最终裁决和高风险 review；
- 任务账本、receipt、worktree/terminal lock 和可观察性比 agent 数量更重要。

社区信息质量不一，因此这里只把重复出现的失败模式作为设计线索，不把单个博客的性能数字当作项目结论。

---

## 8. 当前推荐的实验架构

### 8.1 总体结构

```text
MiMo Code / OpenCode Harness
  ├── Context Bootstrapper
  ├── Task Classifier / Domain Owner Router
  ├── Deterministic Workflow Runtime
  ├── Parallel Research / Review Waves
  ├── Checkpoint Writer
  ├── Project Memory / Session Memory
  ├── Goal Verifier
  ├── Evidence Collector
  ├── Writer / Worktree Gate
  ├── Session Tree / Run Viewer
  └── Dream / Distill
        ↓
OpenCode / Codex / Claude CLI workers
        ↓
ArknightsInfraCalc Rust domain engine
```

OpenCode 不需要被舍弃。它可以继续作为：

- 交互入口；
- TUI/Web 界面；
- 本地工具和权限执行面；
- worker session；
- 人类接管和审批入口。

MiMo Code 可以作为第一候选实验 harness，而不是在项目外另造一套 LangGraph + OpenCode 双运行时。

### 8.2 推荐 agent 角色

首批不追求大量角色，建议：

```text
orchestrator / root
  负责拆解、wave、汇总和下一步决策，不直接承担所有实现

domain-explorer
  读取代码、canonical 和调用链，只读

evidence-researcher
  搜索外部资料、执行验证、整理来源，只读

executor
  唯一默认 writer，按计划实现和测试

reviewer
  新鲜上下文独立审查，不能直接修改主工作树

memory/checkpoint-writer
  独立维护 session checkpoint 和候选 project memory
```

### 8.3 推荐主 workflow

```text
bootstrap_context
→ classify_task
→ route_domain_owner
→ plan / plan-review
→ parallel:
     domain explorer
     canonical/document researcher
     evidence/test mapper
→ join findings
→ select one implementation unit
→ single-writer executor
→ deterministic evidence verification
→ parallel:
     correctness reviewer
     regression reviewer
     evidence reviewer
→ repair if needed
→ final verification
→ checkpoint
→ memory capture
→ report
```

### 8.4 Model路由建议

这只是待验证 policy，不是已裁决事实：

```text
GPT-5.6 Sol
  架构裁决、复杂 debug、最终 synthesis、Goal verifier

GPT-5.6 Terra
  代码调查、实现、一般 review、外部研究

GPT-5.6 Luna
  文件提取、分类、证据整理、memory checkpoint、状态同步
```

必须通过代表性任务集测量：正确性、完整性、证据覆盖、token、latency、cost、重试率和卡住率。

---

## 9. 当前 Evidence 设计的复盘

### 9.1 当前能力

项目已有 evidence 体系，主要由以下部分组成：

- `scripts/codex/run_evidence.sh`：包装命令，记录 cwd、命令、输入、stdout/stderr、时间、耗时、exit code 和 PASS/FAIL；
- `target/codex-runs/<task>/manifest.json`：聚合 runs、artifacts、输入和任务元数据；
- `check_task_scope.py`：检查实际 changed paths 是否超出任务范围；
- `render_evidence.py`：检查 manifest、status、日志和登记产物，并渲染验证报告；
- `compare_test_failures.py`：比较完整 baseline/current 的失败名称集合；
- `docs/QUALITY_AND_AUDIT.md`：规定风险匹配证据、solver assurance、真实 CLI、full suite 和未验证风险。

当前体系的强项是：

- 证据可复现；
- 不覆盖旧运行，保留重复尝试；
- manifest 和 artifact 可追溯；
- scope、失败集合和未跑类别有机械检查；
- 不把脚本结果冒充领域语义或全局最优证明。

### 9.2 为什么使用体验仍然觉得“没用”

当前 evidence 更像**交付审计附件**，不是**Agent 工作控制面**：

```text
当前：先运行命令 → 保存日志 → 最后渲染报告
缺少：任务开始前规划证明义务 → 运行中更新缺口 → 阻止错误完成
```

具体缺口：

1. `run_evidence.sh` 主要记录命令和退出码，不理解 required anchor、候选集合、global effect、solver guarantee 或 CLI 语义。
2. evidence 通常在任务末尾才被读取，不能主动告诉 agent 下一步应跑什么。
3. 任务 scope、canonical 不变量、runs、reviewer 判断、progress 和 memory 没有被一个持续任务对象统一驱动。
4. agent 不会自动知道哪个 artifact 是当前 commit 的 baseline、哪个是新增失败、哪个证明义务仍为 `pending`。
5. “命令通过”与“业务结论成立”之间仍需要主 Agent、domain reviewer 和 canonical 语义判断，当前没有统一的 proof-gap 状态。

因此问题不是 evidence 日志工具没有价值，而是它的责任定位偏窄：

```text
现状：命令留痕 + 交付检查
目标：任务证明系统 + Agent 工作反馈
```

### 9.3 Evidence Control Plane 方向

不废弃现有 `run_evidence.sh`，在其上增加任务级 Evidence Control Plane：

```text
Task Contract
  ↓
Proof Obligations
  ↓
Evidence Runs
  ↓
Assertions / Comparisons
  ↓
Reviewer Decision
  ↓
Completion Status
```

任务开始时声明证明义务：

```yaml
task:
  id: cross-facility-scope-fix
  invariant: global atom 不重复计数
  owner: cross_facility

proof_obligations:
  - id: no_double_count
    kind: invariant
    required: true
    status: pending
  - id: real_layout_entry
    kind: cli
    command_profile: layout-team-rotation
    status: pending
  - id: regression
    kind: targeted-test
    status: pending
```

建议增加以下运行时能力：

- `evidence-plan`：根据 maintenance/feature/quality/system-audit 和风险类型生成验证计划；
- `evidence-summary`：把原始日志压缩成机器可读的 obligation 状态、失败集合、artifact 和 reviewer-required 标记；
- `completion-gate`：required proof obligations 未达到 `verified` 时，阻止 Agent 宣称完成；
- `baseline-link`：明确 baseline commit、fixture、policy、seed、输入和当前候选；
- `proof-gap`：显式区分 `verified`、`partial`、`failed`、`blocked`、`not_run`，而不是只看命令 exit code。

边界必须保持：

- Control Plane 可以检查结构、命令、产物、状态和已声明断言；
- 它不能用测试结果推翻 canonical 领域语义；
- 它不能把 Agent 自然语言总结升级为 evidence；
- 它不能自行裁决 solver 的全局最优性；
- domain reviewer 仍负责语义、保证等级和最终风险判断。

### 9.4 与 Harness 的关系

Evidence Control Plane 应成为 AI harness 的主动节点：

```text
bootstrap_context
→ evidence-plan
→ parallel investigation
→ implementation
→ evidence-runner
→ proof-gap update
→ independent review
→ completion-gate
```

这比单独增加 `benchmark-runner` 更优先，因为当前真正的缺口不是日志数量，而是 evidence 没有参与任务调度、缺口发现和完成判断。

第一阶段不需要重写现有脚本，先让 task ledger 引用现有 manifest 和 report；第二阶段再补 obligation summary 与 completion gate。

---

## 10. 必须保留的项目约束

AI workflow 不得绕过以下边界：

1. 业务机制和 solver 继续由 Rust `infra-core` 负责。
2. `build_plan → execute_plan → fill → resolve` 继续是 layout 主路径。
3. AI orchestrator 不得在 CLI 或 agent prompt 中重新实现机制公式。
4. domain canonical 优先于 agent memory、旧输出、fixture 或模型常识。
5. 只读调查可以并行，写入保持单 writer。
6. 并行 writer 使用隔离 worktree，不共享主工作树。
7. `required admission` 不能被 tag、priority、shortcut 或 shift_bind 替代。
8. Candidate/剪枝/目标/Top-K 变化必须标注约束性质，不能未经证明声称全局最优。
9. memory 只能作为导航、偏好、纠正、进度和候选知识层；不能自动覆盖 canonical。
10. 任务结束必须留下 evidence、progress、decision 或明确的未验证风险。

---

## 11. 需要验证的关键未知项

### 11.1 MiMo / OpenCode 兼容性

- 当前 GPT-5.6 provider gateway 是否支持 MiMo 所需的 tool schema、background subagent 和 workflow 行为；
- 项目现有 `AGENTS.md`、Skills、permission 和 custom provider 是否能被 MiMo 正确加载；
- MiMo workflow 是否能调用项目统一 evidence 工具；
- MiMo 的 memory path 是否能与项目工作树、多个 worktree 和团队共享语义兼容；
- MiMo 的 Compose/legacy agent 是否会违反项目单 writer 或文档生命周期规则；
- OpenCode/MiMo child session 是否能稳定获得 parent-child session id、状态和原始 transcript。

### 11.2 GPT-5.6 harness contract

- optional tool parameters 是否产生虚假默认值；
- gateway 是否支持 Responses API Multi-agent beta；
- gateway 是否支持 Programmatic Tool Calling；
- reasoning replay 是否需要 `store: false` 或 self-contained encrypted reasoning；
- prompt cache 是否支持 workspace-scoped key 和显式 breakpoints；
- provider 是否能承受并发 fan-out；
- 多 agent 是否会与现有 OpenCode Task permission 产生重复调度；
- tool 空结果、错误、取消和超时是否都返回结构化状态。

### 11.3 workflow 效果

- 并行是否降低真实 wall-clock，而不是只增加 token；
- memory 是否减少重复事实说明；
- checkpoint 是否能在 context rebuild 后恢复正确任务方向；
- Goal verifier 是否降低过早完成；
- reviewer 是否发现真实遗漏，还是只制造重复噪声；
- 多模型路由是否降低成本而不降低领域正确性；
- 主线摘要、原始 session 和 artifact 是否保持可读。

---

## 12. 建议的实验计划

### 实验 0：只读兼容性

在独立目录或独立 worktree 安装 MiMo Code，使用项目现有 provider，执行一个只读任务：

```text
分别调查 layout 编排主路径、cross_facility owner 和 evidence 验证入口。
要求三个子任务并行，返回文件、行号、事实、未知项和结果 artifact。
```

验收：

- 子任务确实并行；
- 每个任务有状态和结果；
- 主 agent 能汇总；
- 没有修改当前工作树；
- memory/checkpoint 可查看。

### 实验 1：长任务 checkpoint

选择一个不改变业务语义的文档或测试任务，运行超过一个 context cycle，验证：

- checkpoint 是否自动生成；
- 新 session 是否能恢复任务；
- memory 是否减少重复说明；
- 进度文件是否可审阅；
- 主 agent 是否会重复宣称完成。

### 实验 2：项目 workflow 适配

实现项目专用 workflow：

```text
maintenance / feature / quality
→ owner skill
→ parallel read-only research
→ plan review
→ one writer
→ evidence
→ reviewer
→ lifecycle update
```

暂时只允许文档、测试或 agent 配置相关的低风险任务。

### 实验 2A：Evidence Control Plane

在不改变现有证据命令实现的前提下，为一个低风险任务增加：

```text
task contract
→ proof obligations
→ 现有 run_evidence manifest
→ obligation summary
→ completion-gate
```

验收：

- Agent 在任务开始时能看到应完成的证明义务；
- 每个运行结果能关联到具体 obligation；
- 缺少真实 CLI、定向回归或 artifact 时状态为 `not_run`/`blocked`；
- 命令通过但语义 reviewer 未完成时不能标记整体 `verified`；
- 不影响现有 evidence 日志和 scope checker；
- 原始日志仍可追溯。

### 实验 3：GPT-5.6 对照

同一任务分别比较：

- 当前 OpenCode 原生 workflow；
- MiMo Code 默认 workflow；
- MiMo Code 项目专用 workflow。

记录：

- 正确性；
- 完整性；
- evidence 覆盖；
- 事实重复提问次数；
- 总 token；
- wall-clock；
- tool calls；
- retry；
- 卡住/等待用户次数；
- 人工接管次数；
- 结果和原始过程的可读性。

### 实验 4：正式业务变更

只有前面实验通过后，才允许把 MiMo workflow 用于涉及以下内容的任务：

- `orchestration_rules.json`；
- `build_plan` / `execute_plan`；
- cross-facility scope；
- schedule/rotation；
- candidate pruning、objective 或 top-K；
- solver assurance。

必须由 `arknights-system-audit` 和 `arknights-evidence` 共同验证，并保留行为对照和未验证风险。

---

## 13. 当前结论

### 已确认

1. 当前项目已有成熟的业务编排层，AI 编排不能替换它。
2. OpenCode 的体验缺口主要是 AI workflow runtime、memory 和可观察性，而不是 subagent 数量。
3. 只靠 prompt 不能可靠保证并行、join、checkpoint、writer lock 和完成验证。
4. Claude Code、Cursor、Codex、MiMo 和生产团队分享都显示：仓库知识、增量进度、独立验证、隔离 workspace 和确定性 gate 是核心能力。
5. GPT-5.6 的并行、长任务和工具能力使更强 harness 的投入回报提高。
6. MiMo Code 已经把动态 workflow、checkpoint、memory、goal verifier、dream/distill 做成开源可运行基线，值得尝试。
7. OpenCode 不需要被舍弃；可以作为交互入口、worker 执行面和人工接管面。
8. 当前 evidence 设计的底层留痕能力是有价值的，但主要是交付审计，不足以作为 Agent 的主动工作控制面。
9. Evidence Control Plane 应成为 harness 的一部分：用 proof obligations、baseline、proof-gap 和 completion gate 连接任务、运行、reviewer 和最终结论。

### 当前推荐

```text
MiMo Code 作为第一候选实验 harness
+ 项目专用 workflow / skills / memory policy
+ GPT-5.6 分层模型路由
+ OpenCode 兼容入口和 worker
+ 现有 Rust domain engine 保持不动
+ canonical/evidence/system-audit 作为硬约束
```

### 尚未裁决

- 是否使用 MiMo 默认 Compose、`compose-next` 还是自定义 workflow；
- 是否把 MiMo 作为 OpenCode 主环境，还是只作为独立实验环境；
- 是否需要 OpenAI Responses API 原生 Multi-agent；
- 是否需要 LangGraph/Mastra/Temporal；
- project memory 如何与 canonical docs、worktree 和团队共享边界对齐；
- 何时允许 writer agent 自动提交或自动合并。

### 暂缓

- 不立即重写现有 Rust 业务编排；
- 不立即引入 Temporal；
- 不立即把所有任务改成 multi-agent；
- 不默认开启 GPT-5.6 Max/Ultra 或 MiMo Max Mode；
- 不把模型自然语言总结作为 evidence；
- 不让自动 memory 覆盖 canonical；
- 不在没有对照数据前宣称全局效率或全局最优。

### 维护规则

后续本主题继续讨论时，新增内容默认追加到本文，而不是只留在聊天记录中。每次更新至少标明：

- 用户明确裁决；
- 仓库当前事实；
- 外部来源事实；
- Agent 推断或建议；
- 未决问题和下一步实验。

如果讨论已经改变项目持久决策、canonical 语义、active workflow 或任务生命周期，再单独更新对应 owner 文档；本文只保留讨论脉络和跨主题汇总。

---

## 14. 第一阶段改造建议

### 14.1 起点判断

第一阶段不应先重写 OpenCode、接入 LangGraph/Temporal，或一次性实现完整 Agent Team。最先需要建立的是一个可测量的 **Harness Spine**：

```text
Task Contract
  → Context Bootstrap
  → Proof Obligations
  → 一次只读并行 wave
  → 结构化结果
  → Evidence Summary
  → Completion Gate
```

原因：当前最大未知不是“能不能再增加 agent”，而是新 harness 是否真的减少重复探索、提高并行吞吐、降低错误完成，并且不破坏项目的 canonical、single-writer 和 evidence 边界。

### 14.2 推荐的首个实现切片

先做一个独立实验目录或 sibling harness 项目，不修改 `infra-core` 业务代码。首个切片只包含：

1. 固定的 `TaskRecord`：任务、commit、项目 profile、owner、状态、子任务、session、artifact 和 proof obligations。
2. `Context Bootstrap`：启动时读取 `AGENTS.md`、相关 canonical owner、当前任务和已有 evidence summary。
3. `research-wave`：一次接收 2～4 个只读任务并行执行，禁止写入主工作树。
4. 结果合同：每个子任务返回 `findings`、文件/行号、未知项、风险和 artifact 路径。
5. `Evidence Summary`：复用现有 `run_evidence.sh` 的 manifest/log，不重写底层记录器，只补 obligation 状态。
6. `Completion Gate`：required obligation 仍为 `pending`、`not_run` 或 `blocked` 时，不允许将任务标为 verified。

### 14.3 首个 workflow

使用当前项目最安全、最能体现价值的只读任务：

```text
用户任务
  → Context Bootstrap
  → 并行调查：
       domain/canonical owner
       Rust 调用链
       evidence/验证入口
  → Join
  → 输出带证据的调查报告
```

第一阶段不做自动实现、不做并行写入、不做自动 merge、不做 solver 语义裁决。

### 14.4 对照实验

同一组 6～8 个任务比较：

```text
A. 当前 OpenCode
B. MiMo 默认 workflow
C. MiMo/project research-wave
```

至少记录：

- 是否实际并行；
- wall-clock；
- token 和 tool calls；
- 重复事实提问次数；
- canonical owner 命中率；
- 文件/行号证据完整度；
- checkpoint/memory 恢复成功率；
- 人工接管次数；
- 主线和原始 session 可读性。

### 14.5 第一阶段停止条件

只有以下结果出现后，才进入单 writer executor：

- 并行 wave 在代表性只读任务上稳定运行；
- 失败、取消和超时有明确状态；
- 主 agent 能读取结构化结果而不需要重新翻全部 transcript；
- evidence obligation 能明确显示 `verified`、`partial`、`blocked` 和 `not_run`；
- 不修改主工作树，原始 session 和 artifact 可追溯；
- 至少有一项可观测收益，例如重复提问减少、调查 wall-clock 降低或证据覆盖提高。

通过后再按顺序增加：

```text
单 writer executor
  → worktree / writer lease
  → reviewer wave
  → benchmark/resource manager
  → project memory promotion
  → Dream/Distill
```

### 14.6 当前建议的唯一第一步

建立一个不触碰业务代码的 **harness protocol/preflight**，先冻结 `TaskRecord`、child event/result、evidence overlay、workspace/resource lease 和 capability matrix；随后再建立 `research-wave` 原型，并用三个真实只读调查任务跑出 A/B 基线。这样先验证协议和适配边界，再验证并行、任务状态、结果可读性、Context Bootstrap 和 Evidence Control Plane。

### 14.7 其他 harness 的尝试价值复评

MiMo Code 仍是目前最完整、最贴近本项目诉求的开源集成基线，但不应因此把它当作唯一候选。按“对本团队实验的边际价值”排序：

| 候选 | 尝试价值 | 最适合验证的问题 | 主要代价/风险 |
|---|---:|---|---|
| MiMo Code | 最高 | checkpoint、分层 memory、动态 workflow、goal verifier、OpenCode 兼容 | fork 绑定、项目规则适配、默认 Compose 可能过重 |
| Claude Agent SDK + Dynamic Workflows | 很高 | 成熟 coding harness、fan-out/synthesize、adversarial verification、worktree、跨 session agent loop | Claude API 绑定；需要独立 runtime；不能直接代表 GPT-5.6 行为 |
| OpenAI Responses Multi-agent / Agents SDK | 很高 | GPT-5.6 原生并行、hosted agent tree、tool calling、WebSocket、模型路由 | Multi-agent beta；memory/workspace/evidence 仍需自建；provider gateway 兼容性未知 |
| oh-my-openagent | 高 | OpenCode 内的实用 orchestrator、Team Mode、background agents、模型分类和 session recovery | 高度 opinionated，功能面大；不应直接当作项目架构真源 |
| LangGraph | 中高 | 自定义项目 profile、显式状态图、持久化、人工介入、确定性 gate | 需要自己实现 coding harness、工具/权限/会话/工作区；迁移成本高 |
| Mastra | 中 | TypeScript 服务化、Studio、工具和 agent 产品化 | 更像应用平台；对本地 coding workspace 的现成能力弱于 MiMo/Claude |
| Temporal | 条件高 | 跨进程、跨天、分布式 worker、强恢复和人工审批 | 解决 durable execution，不解决 coding harness；运维成本最高 |

其中两个候选值得加入早期 bake-off：

1. **Claude Agent SDK + Dynamic Workflows**：它代表当前公开的成熟 workflow 方法论。官方公开的可复用模式包括 classify-and-act、fan-out-and-synthesize、adversarial verification、generate-and-filter、tournament、loop-until-done、model routing 和 memory/rule distillation。它适合验证“理想 harness 应该怎样工作”，即使最终不采用 Claude 作为 worker。
2. **OpenAI Responses Multi-agent / Agents SDK**：它代表 GPT-5.6 的原生路径。Multi-agent 负责并行 agent tree，Agents SDK 负责 agent loop、sessions、guardrails、approvals 和 tracing；但项目自己的 canonical、evidence、worktree 和 memory 仍需在应用层建立。它适合验证“GPT-5.6 是否能在原生并行 runtime 中改善当前体验”。

因此对照实验不必只比较当前 OpenCode、MiMo 默认和 MiMo 项目 profile；可以分两轮：

```text
第一轮：
  OpenCode 原生
  MiMo Code
  oh-my-openagent

第二轮：
  MiMo 项目 profile
  Claude Agent SDK Dynamic Workflow
  OpenAI Responses Multi-agent / Agents SDK
```

第一轮回答“现成 coding harness 谁最适合本地项目”；第二轮回答“模型原生 runtime 是否值得成为长期控制面”。LangGraph、Mastra 和 Temporal 暂不进入第一轮，因为它们需要先建设更多基础设施，容易把框架迁移成本误判为 workflow 收益。

当前判断：

```text
MiMo = 最佳开源整合基线
Claude Agent SDK = 最佳 workflow 方法论对照
OpenAI Agents/Responses = GPT-5.6 原生能力对照
oh-my-openagent = 最快 OpenCode 体验增强
LangGraph = 最值得长期自研 profile 的控制流底座
```

### 14.8 Sol 复审结果与修订

Sol 复审结论为 `VERDICT: HOLD`：方向成立，但第一阶段尚未达到可直接实现的冻结程度。复审发现与主 Agent 判断如下。

#### 必须先修正

1. **冻结第一阶段边界**：不能同时把 `Evidence Summary/Completion Gate` 说成第一阶段和第二阶段；不能同时使用“三个任务”和“6～8 个任务”作为首轮范围；MiMo 默认 Compose 包含实现/合并，不适合作为只读第一阶段入口。
2. **Completion Gate 必须 fail-closed**：`verified` 之外的 `failed`、`partial`、`blocked`、`not_run`、`pending` 一律不能通过；evidence overlay 只能引用现有 manifest/artifact，不能改写 evidence 真源。
3. **明确实际 profile 映射**：当前 OpenCode 只允许四个 Task 名称，实际项目 agent 是 Terra/Luna/Sol；现有只读 agent 禁止 bash/task/edit，不能假设它们能够自行执行 evidence 命令。
4. **定义 snapshot、状态和幂等语义**：TaskRecord 必须包含 base SHA、dirty 状态、输入 hash、runtime/profile 版本、worktree、事件日志、重试去重、取消和恢复状态。
5. **先定义私有数据边界**：第一阶段只使用公开仓库材料和公开 fixture；真实 operbox/layout/assignment、远程 provider、transcript 和 memory 的脱敏、保留期、telemetry 策略在协议冻结前不进入实验。

#### 应该补充

- Context Bootstrap 输出 source manifest，记录文档角色、状态、领域键和 hash；遇到 current canonical 冲突必须 `blocked`，不把 archive 或未恢复 proposal 当作稳定事实。
- 结果合同增加 claim type、source layer、canonical conflict、guarantee class、negative evidence 和 reviewer-required。
- CPU/RAM/IO 资源门禁从第一阶段就存在；如果执行 cargo、plan、layout 或 benchmark，solver/benchmark 并发默认 1；否则首轮只做静态只读调查。
- 框架比较拆层：宿主/交互、agent loop、workflow runtime、durable state、sandbox/权限、evidence adapter、provider compatibility、数据/telemetry 和退出成本。
- 先冻结 harness-neutral protocol，再为 MiMo、OpenCode、Claude SDK、OpenAI runtime 编写 adapter；不让某个框架 API 成为项目真源。
- 实验指标增加事实/引用准确率、canonical owner 命中错误、Gate false-pass、恢复后的重复执行、资源峰值和隐私泄漏。

#### 可延后

- Dream/Distill、自动 memory promotion、自动 merge、完整 session viewer 和完整 writer lease；
- LangGraph、Mastra、Temporal 的正式接入；
- `executor`、`performance-engineer` 等新角色的完整目录；首轮只映射当前 Terra/Luna/Sol profile。

#### 修订后的 Phase 0

```text
Phase 0A：协议冻结
  TaskRecord
  CapabilityMatrix
  ChildEvent / ResultContract
  EvidenceOverlay
  Snapshot / ResourceLease

Phase 0B：公开材料 preflight
  只读仓库代码与 canonical
  不调用私有 operbox/assignment
  不执行写入、benchmark 或真实 plan
  验证三个 runtime adapter 能否表达任务状态和结果

Phase 0C：research-wave
  2～4 个只读任务
  仅使用实际 Terra/Luna/Sol 映射
  结构化 join
  fail-closed completion gate
```

只有 Phase 0C 稳定后，才进入单 writer executor、worktree lease、review wave 和 CPU 资源 runner。

### 14.9 分阶段实施计划

本计划按“先证明控制面可行，再扩大权限和任务半径”的顺序执行。每一阶段都有独立产物和停止条件；未通过前不进入下一阶段。

#### Phase 0A：冻结 harness-neutral protocol

目标：不依赖 MiMo、OpenCode、Claude 或 OpenAI 的专用 API，先定义可移植合同。

产物：

- `TaskRecord`：任务身份、project profile、base SHA、dirty 状态、输入 hash、runtime/profile version、状态、依赖和 artifact；
- `CapabilityMatrix`：runtime 是否支持 spawn、parallel、wait、cancel、resume、structured output、worktree、sandbox、trace 和 privacy policy；
- `ChildEvent`：queued/running/progress/waiting/succeeded/failed/cancelled/expired；
- `ResultContract`：findings、claims、source layer、文件/行号、unknowns、negative evidence、guarantee class、reviewer-required 和 artifact；
- `EvidenceOverlay`：只引用现有 manifest/status/log/artifact，不改写 `run_evidence.sh` 真源；
- `Snapshot/ResourceLease`：base SHA、允许目录、私有目录、CPU/RAM/IO class、并发上限和释放语义；
- 私有数据分类：公开材料、项目内部材料、私有编制、远程 provider 禁止项和保留期。

验收：schema 能表达一次成功、失败、取消、重试、恢复和阻塞任务；所有未支持能力能返回 `unsupported` 或 `blocked`，不能模拟成功。

#### Phase 0B：公开材料 compatibility preflight

目标：只验证 runtime/adapter 能否表达协议，不运行真实业务任务。

比较对象：

```text
当前 OpenCode Task
MiMo Code workflow interface
Claude Dynamic Workflow / Agent SDK interface
OpenAI Multi-agent / Agents SDK interface
```

输入限制：只读仓库代码、canonical 文档和公开 fixture；不使用私有 operbox/assignment，不运行真实 `plan`、benchmark 或写入。

验收：每个 adapter 能创建 TaskRecord、发出 child events、返回结构化 result、记录 capability gap，并保留原始 session/artifact 引用。不能接入的 runtime 记录为 `unsupported`，不阻塞其他 adapter。

#### Phase 0C：research-wave MVP

目标：在不触碰业务代码的前提下验证真实并行体验。

固定三个任务：

1. 调查 layout 编排主路径；
2. 调查 cross_facility 当前 owner；
3. 调查 evidence 验证入口和 proof gap。

运行规则：

- 2～4 个只读 child，映射到现有 Terra/Luna/Sol profile；
- child 禁止 edit、bash、task 和主工作树写入；
- 主 orchestrator 只负责 dispatch、join、去重和汇总；
- required result 未完成时 completion gate fail-closed；
- 不执行 cargo、plan、layout、benchmark；
- 结果必须包含文件/行号、事实/推断/未知分类和 artifact。

验收指标：实际并行、wall-clock、重复提问次数、canonical owner 命中率、结果结构完整度、session 可读性、失败/取消状态和 artifact 可追溯性。

#### Phase 1：Evidence Control Plane

目标：让 evidence 从交付附件变成 workflow 的主动反馈。

范围：

- task ledger 读取 proof obligations；
- evidence summary 将 run manifest 映射到 obligation；
- baseline/current 失败集合和输入 hash 关联；
- `verified/partial/failed/blocked/not_run/pending` 状态；
- completion gate allow-list；
- 只有所有 required obligation 为 `verified` 才允许整体通过。

仍不运行私有业务 fixture，不修改现有证据脚本语义。

#### Phase 2：项目 profile 与低风险 writer

目标：把通用 harness 接入当前项目 workflow，但暂不修改 solver 核心。

首个 profile：

```text
domain: solver
workload: cpu_bound
readonly_wave: 2-4
writer_count: 1
benchmark_concurrency: 1
```

只允许文档、测试、agent 配置和 evidence 工具相关的低风险写入。写入前必须获得 writer lease；失败时保留 worktree 和 receipt，不自动 merge。

#### Phase 3：隔离 writer、review wave 和资源 runner

目标：扩大到真实工程变更。

- worktree 创建与 base SHA 固定；
- writer lease、过期、释放和崩溃恢复；
- 新鲜上下文 reviewer wave；
- `cargo fmt`、targeted test、full suite、真实 CLI 的 evidence profile；
- cargo/solver/benchmark resource class，benchmark 并发固定 1；
- 私有 operbox/layout/assignment 的本地 evidence 目录和脱敏摘要。

只有这一步之后，才允许触碰跨设施、required admission、schedule/rotation、候选剪枝、objective、Bake/cache 和 solver assurance 任务。

#### Phase 4：框架 bake-off 与长期选型

使用同一 protocol、同一公开任务集和同一指标进行两轮对照：

```text
第一轮：OpenCode 原生 / MiMo / oh-my-openagent
第二轮：MiMo project profile / Claude Dynamic Workflow / OpenAI Multi-agent
```

LangGraph、Mastra、Temporal 只有在实验显示当前 runtime 的 durable state、服务化、跨进程或多 worker 能力不足时才进入候选实现。

选型不只看完成率，还必须比较：任务质量、证据覆盖、Gate false-pass、隐私边界、资源峰值、恢复/重复执行、可读性、token、wall-clock、退出成本和 provider 绑定。

#### Phase 5：memory promotion 与持续演化

目标：最后再启用 checkpoint writer、project memory promotion、Dream/Distill 和定期 workflow evaluation。

memory 只负责偏好、纠正、进度、导航和候选知识；领域事实仍必须晋升到 canonical/ADR/change，由 lifecycle 和 reviewer 处理。

### 14.10 实施顺序的最终裁决

```text
先协议，不先选框架
先公开只读，不先接私有数据
先 research-wave，不先写代码
先 evidence overlay，不先重写 evidence runner
先低风险 writer，不先改 solver
先资源/隐私/恢复门禁，不先扩大并发
```

本计划的第一个实际写入单元应是 Phase 0A 的协议草案和 capability matrix；完成后再实现 Phase 0B 的 adapter preflight。

### 14.11 Phase 0A 首批产物

已创建但尚未冻结为 `decision` 的首批协议产物：

- `contracts/agent-harness-v0.schema.json`：TaskRecord、ChildEvent、ResultContract、ProofObligation、Artifact 和 Claim 的 v0 schema；
- `contracts/agent-harness-capability-matrix-v0.json`：当前 OpenCode、MiMo、Claude Agent SDK 和 OpenAI Responses Multi-agent 的能力状态，未验证项明确标为 `unverified`；
- `docs/TODO/AI编排Harness Phase0A协议草案.md`：人类可读协议、边界、不变量、preflight 验收和后续范围；
- `docs/TODO/README.md`：新增 active-change 索引项。

已完成验证：

- 两个 JSON 文件通过 Python JSON parse；
- evidence 记录位于 `target/codex-runs/harness-phase0a/`；
- 未运行 Rust、CLI、benchmark 或真实业务入口；
- 文档生命周期全量检查仍被既有问题阻塞：`docs/INDEX.md` 与 TODO 生成区漂移、两个既有 `in-progress` 任务、三份既有非法元数据。上述阻塞未由本轮修改清理。

当前状态仍为 `proposal`，下一步是 Phase 0B adapter preflight，不是直接接入私有数据或启动 writer。

### 14.12 Phase 0B adapter preflight 结果

Phase 0B 已完成首个静态 preflight 工具：

- `scripts/codex/preflight_harness.py`：只读读取协议 schema、capability matrix 和当前项目 `.opencode/agents/`，检查协议版本、默认 fail-closed policy、实际 profile 存在性和 `edit/bash/task=deny`；
- `scripts/codex/tests/test_preflight_harness.py`：锁定当前三个项目本地只读 profile 和 preflight scope；
- 报告：`target/codex-runs/harness-phase0a/preflight/phase0b-rerun.json`。

第一次 preflight 发现 capability matrix 错误地把全局允许但项目本地不存在的 `plan-reviewer` 列为实际 profile；已移除该条目。这个失败说明 preflight 发现了真实配置漂移，而不是只验证 JSON 能解析。

当前 preflight 通过的事实：

- `terra-explorer`、`luna-extractor`、`sol-reviewer` 是实际项目本地 profile；
- 三者均为 `subagent`，且 `edit`、`bash`、`task` 均为 `deny`；
- Phase 0B 没有写入、私有输入、业务命令或 runtime 启动；
- MiMo、Claude Agent SDK、OpenAI Responses Multi-agent 仍明确标记为 `unverified`，没有被冒充为可用；
- Phase 0B preflight 和 targeted test 均通过 evidence wrapper 留痕。

这不是 runtime compatibility 通过；它只证明协议和当前本地 profile 的静态边界可检查。下一步仍是公开材料 adapter preflight，随后才是实际 `research-wave`。

### 14.15 Phase 0C runtime adapter probe 结果

已完成本地环境探针，范围严格为命令发现和 SDK import：不启动 runtime、不联网、不读取私有输入、不写入项目工作树。

报告：[runtime-adapter-probe.json](../target/codex-runs/harness-phase0c/runtime-adapter-probe.json)

当前事实：

- `opencode` CLI：通过用户提供的绝对路径确认可用，版本 `1.18.19`；项目 `opencode.json` 存在，但本地 SDK import 探针未通过；
- `mimo` CLI：当前 API/子进程 `PATH` 未发现；同样不等于用户交互 shell 中不存在；
- Claude Code CLI：`2.1.181` 可发现，但 Claude Agent SDK 未安装；
- OpenAI Responses Multi-agent：未验证，未发起 API 请求；
- 所有 runtime probe 均标记 `launch=false`，没有把本地工具发现结果升级为 runtime 兼容性结论。

OpenCode 现在是 Phase 0C 的首选本地 adapter backend。已通过真实 `opencode run --agent
terra-explorer --format json` 启动只读 child session 并返回带文件/行号的结构化结果；原始
worker log 和 derived summary 已登记到 `target/codex-runs/harness-phase0c/`。仍未验证
多 child 并发、child session resume/cancel 和外部 SDK import。MiMo 仍可通过
`HARNESS_MIMO_BIN=/绝对路径/mimo` 显式加入，不把 PATH 不可见误报为 CLI 不存在。

### 14.13 Phase 0C research-wave 结果

已执行三个真实只读 child：

```text
layout-owner           terra-explorer  succeeded
cross-facility-owner   terra-explorer  succeeded
evidence-entry         luna-extractor  succeeded
```

wave 结果 artifact：

- [research-wave-0c-join.md](../target/codex-runs/harness-phase0c/reports/research-wave-0c-join.md)
- [research-wave-0c 计划](../target/codex-runs/harness-phase0c/research-wave-0c.json)

三个 child 均遵守公开只读范围并返回了文件/行号、claims、unknowns 和 negative evidence。join 没有通过，而是被正确标记为 `blocked`，原因是 cross-facility 调查发现当前文档与代码存在未裁决冲突：Global atom 的 per-room 语义、StateConvert 扣回、FacilityLevel selector 扣回和 P2/P3 迁移状态。

这证明 Phase 0C 的 fail-closed 目标有效：

```text
all_children_return = verified
public_scope_only = verified
join_review = blocked
overall = blocked
```

这些 cross-facility 问题属于 research-wave 选取的调查内容，不是 harness 改造的前置条件。它们应作为独立 domain side finding 进入 `arknights-system-audit`，但不阻塞 harness 协议、adapter 或只读 wave 的继续开发；在用户或 canonical owner 裁决前仍不得修改其代码语义。

### 14.14 Cross-facility 只读审计结果

有界审计已完成，报告保存在 [cross-facility-audit.md](../target/codex-runs/harness-phase0c/reports/cross-facility-audit.md)。审计确认：

- manufacture 和 trade interpreter 都没有按 `AtomScope` 跳过 Global atom；
- Global `StateConvert` 如果未来被标记为 Global，可能先由 cross-facility 执行，再由 room interpreter 执行，且当前扣回只覆盖部分 `StateProduce`；
- `FacilityLevel` / `FacilityLevelMinusOne` 的全局执行存在，但 room deduction 当前返回 `0.0`；
- 当前数据找到的是 room-scope blackkey/rosemary `StateConvert`，没有找到现存 Global `StateConvert` 实例；
- `global_resource::run_conversions` 是另一套注册表驱动、会消耗源资源的转换机制。

用户已裁决核心语义：Global atom 只由全局层执行一次，per-room 跳过，不再执行后扣回。
因此以下两项冲突已关闭：Global atom 的 per-room 语义、FacilityLevel 类扣回是否需要保留。

仍待裁决：

1. Global `StateConvert` 是否允许，以及它是共享非消耗还是消耗式转换；
2. P2/P3 文档状态如何与当前迁移事实统一。

根据 `arknights-system-audit` 和文档生命周期规则，Global atom 的用户裁决已经足够支持
后续 harness 实验；Global `StateConvert` 和迁移状态口径属于独立 domain side finding，
不应阻塞 harness 改造，也不能借 harness 任务授权修改相关转换语义或声称 P2/P3 已完成。
Harness 改造已验证了“child 成功但 semantic join blocked”的 fail-closed 路径；后续 join
应允许把该领域结果标记为 `blocked` 并继续报告整体 harness 运行状态。

---

## 15. 外部参考索引

### 官方与工程团队

- [OpenAI: GPT-5.6](https://openai.com/index/gpt-5-6/)
- [OpenAI: Harness engineering](https://openai.com/index/harness-engineering/)
- [OpenAI Responses API: Multi-agent](https://developers.openai.com/api/docs/guides/responses-multi-agent)
- [OpenAI Responses API: Programmatic Tool Calling](https://developers.openai.com/api/docs/guides/tools-programmatic-tool-calling)
- [OpenAI Agents SDK](https://developers.openai.com/api/docs/guides/agents)
- [Anthropic: Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)
- [Claude Code: Dynamic workflows](https://claude.com/blog/a-harness-for-every-task-dynamic-workflows-in-claude-code)
- [Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview)
- [Claude Code memory](https://code.claude.com/docs/en/memory.md)
- [Claude Code parallel agents](https://code.claude.com/docs/en/agents.md)
- [Ploy: Migrating a production AI agent to GPT-5.6](https://ploy.ai/blog/migrating-a-production-ai-agent-to-gpt-5-6)

### Harness / workflow 项目

- [XiaomiMiMo/MiMo-Code](https://github.com/XiaomiMiMo/MiMo-Code)
- [MiMo Code long-horizon design](https://mimo.xiaomi.com/blog/mimo-code-long-horizon)
- [OpenCode background agents](https://github.com/kdcokenny/opencode-background-agents)
- [OpenCode Arise](https://github.com/bluelovers/opencode-arise)
- [oh-my-openagent](https://github.com/code-yeongyu/oh-my-openagent)
- [CrewBee](https://github.com/CrewBeeLab/CrewBee)
- [FlowDeck](https://github.com/DVNghiem/FlowDeck)
- [Context Drop](https://github.com/mupt-ai/context-drop)
- [Agent Log Viewer](https://github.com/Latand/live-log-viewer-next)

### 社区入口

- [Hacker News: MiMo Code is now released and open-source](https://news.ycombinator.com/item?id=48490826)
- [HN search: GPT-5.6 agent](https://hn.algolia.com/?query=GPT-5.6%20agent)
- [HN search: AI coding agent orchestration](https://hn.algolia.com/?query=AI%20coding%20agent%20orchestration%20subagents)
### 14.16 OpenCode CLI adapter 结果

使用用户提供的绝对路径 `/home/knightcode/.nvm/versions/node/v24.18.0/bin/opencode` 确认版本为 `1.18.19`。

首个 `terra-explorer` headless 运行暴露了一个 adapter 边界：`opencode run --agent terra-explorer`
会提示该 agent 是 subagent、不能作为 primary，然后回退到默认 agent；三路尝试因此超时。
这不是并行能力失败，而是 profile 映射错误，adapter 必须 fail-closed，不能接受静默 fallback。

随后使用内置 `plan` primary 进行三路并行 CLI 验证，结果全部成功：

```text
layout      succeeded
cross       succeeded
evidence    succeeded
```

报告：[opencode-plan-wave-join.md](../target/codex-runs/harness-phase0c/reports/opencode-plan-wave-join.md)

已验证：OpenCode CLI subprocess 可作为外部 adapter，支持三路独立 JSON event stream 和只读结果返回。
未验证：项目 subagent profile 直接透传、child session resume/cancel、外部 SDK import 和领域语义结论。

### 14.17 Codex CLI 作为 GPT worker 的候选

用户提出：与其让 OpenCode headless 调用项目 subagent，不如由 Codex CLI 调用 GPT 作为 worker。这个方向值得优先验证。

初步判断：**Codex CLI 作为 GPT worker 可能比 OpenCode CLI 更合适，但不等于 Codex 应接管整个 orchestrator。**

推荐职责：

```text
Harness / Orchestrator
  ├── TaskRecord、wave、join、evidence、资源和隐私门禁
  └── worker adapter
        ├── Codex CLI → GPT worker
        ├── OpenCode CLI → plan/read-only worker
        └── Claude CLI → 可选对照 worker
```

可能的优势：

- Codex 的 primary/worker 模型路径可能比 OpenCode 的 subagent profile headless 透传更直接；
- GPT worker 使用自己的 agent loop、sandbox、session 和工具契约；
- Codex 与 GPT-5.6 的 provider/model 组合更自然，减少 OpenCode provider gateway 的额外兼容变量；
- 可把 GPT worker 作为独立 CLI subprocess，容易纳入 TaskRecord、超时、退出码、日志和 worktree；
- 可以直接比较 GPT worker 与 OpenCode plan worker，而不把 OpenCode subagent fallback 误判成模型能力。

必须验证的风险：

- 当前机器是否能从 API/worker 运行环境发现 Codex CLI；
- Codex headless 命令、JSON 输出、session id、resume/cancel 和 sandbox 参数；
- Codex 是否能以只读权限运行，并把所有写入拒绝在 worker 层；
- Codex 的 `AGENTS.md`、`.codex/agents`、skills 和当前项目 canonical 加载顺序；
- GPT worker 的输出是否能映射到 ResultContract，尤其是 claims、来源、unknowns 和 artifact；
- Codex session/transcript 是否能被外部 harness 可靠索引；
- provider 计费、缓存、并发限制和 telemetry 是否与 OpenCode worker 可公平比较。

因此下一轮 bake-off 优先比较：

```text
OpenCode plan primary worker
Codex GPT worker
Claude CLI worker（可选）
```

使用同一个 `agent-harness-v0` TaskRecord、其 `task.research_wave` specialization、同一组三个公开只读任务和同一 ResultContract。只有 adapter、权限、日志和退出状态都可比时，才比较模型质量和速度。

当前不改变长期选型：Codex 只是新增的高价值 worker adapter 候选；MiMo 仍是完整 coding harness 基线，OpenCode 仍是现有交互和本地工具入口。

### 14.18 本地 API 中转与 OpenCode subagent 的组合判断

用户进一步提出：是否在本地搭建 API 中转服务进行路由编排，或直接让 OpenCode 调用 subagent。

这里需要拆成两个正交问题：

```text
模型路由层：这次请求由哪个 provider/model 处理？
任务编排层：任务如何拆分、并行、等待、恢复、汇总和验收？
```

本地 API gateway 擅长前者，OpenCode subagent/harness 擅长后者的一部分；不能用 gateway 代替 workflow runtime，也不能用 subagent 代替 provider routing。

#### 方案比较

| 方案 | 强项 | 弱项 | 当前判断 |
|---|---|---|---|
| 直接 OpenCode subagent | 交互体验、上下文和本地工具最自然 | 并行/恢复主要由模型决定；headless profile 透传当前有 gap | 保留为交互 worker |
| 本地 API gateway | provider 路由、模型分类、限流、缓存、成本和 trace | 不负责 TaskRecord、wave、join、worktree 或 memory | 值得建设，但不是 orchestrator |
| 外部 orchestrator + OpenCode/Codex workers | 确定性 wave、状态、证据和多 worker | 需要 session/UI/权限适配 | 长期目标 |
| OpenCode plugin 内部完成全部编排 | UX 一体化 | 受 OpenCode runtime/API 边界限制 | 作为轻量入口 |

#### 推荐组合

```text
本地 Gateway
  ├── provider/model routing
  ├── workspace/project/model policy
  ├── cache / rate limit / cost
  ├── normalized trace
  └── 不修改工具语义、不代替任务状态

Harness / Orchestrator
  ├── TaskRecord
  ├── research-wave / join
  ├── proof obligations / completion gate
  ├── writer/worktree/resource lease
  └── 调用 worker adapter

Workers
  ├── OpenCode plan primary
  ├── Codex GPT worker
  └── Claude CLI / MiMo 对照 worker
```

gateway 的第一版应尽量是透明 pass-through：不改写 tool schema、不吞错误、不把 provider fallback 伪装成成功。Ploy 的 GPT-5.6 迁移经验表明，工具参数、prompt cache 和 reasoning replay 的 provider 差异需要在 adapter boundary 显式验证，不能用一个“兼容 OpenAI”的字符串转发器掩盖。

#### 当前决策

1. 不把本地 gateway 当作 harness 的替代品。
2. 不放弃直接 OpenCode subagent；它仍是交互模式下的低成本路径。
3. 将 gateway 设计为独立可插拔层，先服务模型路由、预算和 trace。
4. 先完成 OpenCode plan primary 和 Codex GPT worker 的 adapter 对照，再决定 gateway 是否进入执行链。
5. gateway 接入后，所有 worker 仍必须返回统一 `ResultContract`；gateway 不拥有 canonical、TaskRecord 或 evidence completion gate。

### 14.19 “本地中转”语义澄清：Worker Broker / MCP Tool Service

用户澄清：所谓本地 API 中转，不是把模型请求做 provider proxy，而是让根 agent 通过工具调用发起 subagent/worker，从而自由选择 worker 的工作方式。

正确抽象是 **本地 Worker Broker**，可以通过 MCP 或 OpenCode plugin tool 暴露：

```text
OpenCode root agent
  → spawn_wave / spawn_worker tool
  → local Worker Broker
      ├── OpenCode CLI worker
      ├── Codex CLI GPT worker
      ├── Claude CLI worker
      └── MiMo workflow worker
  → TaskRecord / ResultContract / artifact
  → root agent
```

这不是普通模型 gateway，而是一个拥有本地进程、session、资源和 artifact 生命周期的 worker orchestration adapter。它比直接让 OpenCode root 调原生 subagent 更有价值，因为：

- root agent 仍然在 OpenCode 对话中，任务可读性不丢；
- worker backend、模型、profile 和权限可以按任务选择；
- OpenCode、Codex、Claude、MiMo 可以共存，不被单一 host 锁定；
- broker 可以统一处理并行、超时、取消、退出码、日志和 ResultContract；
- OpenCode subagent profile 不能 headless 透传的问题被隔离在具体 adapter，而不是污染主 workflow。

首批工具建议：

```text
spawn_wave(tasks[])
wait_wave(wave_id)
read_worker_result(task_id)
cancel_worker(task_id)
worker_capabilities()
```

`tasks[]` 必须一次性提交，避免 root agent 逐个调用导致串行。每个 task 显式声明：

```text
backend: opencode | codex | claude | mimo
profile/model
prompt
write_policy
data_classification
resource_class
timeout
```

Broker 只执行和记录，不裁决领域语义；它必须拒绝不支持的 capability、拒绝私有数据越界、拒绝静默 fallback，并把 worker 失败和领域 reviewer 的 blocked 分开记录。

因此短期架构调整为：

```text
OpenCode root + Worker Broker tool
  → 先接 OpenCode plan primary 和 Codex GPT worker
  → 再接 Claude/MiMo
  → 统一 TaskRecord、ResultContract、EvidenceOverlay
```

这比先建设独立 LangGraph 服务更适合作为当前项目的第一条可验证路径；LangGraph、Temporal 等仍保留为 broker 内部或后续 durable runtime 的候选。

### 14.20 最小实现：Tool Call Facade，不单独搭建 MCP

用户进一步裁决：当前不需要独立 MCP server；把 subagent/worker launcher 伪装成 root agent 可调用的 tool call 即可。

第一版收缩为：

```text
OpenCode root agent
  → local tool: run_worker / run_workers
  → 直接启动 OpenCode/Codex/Claude CLI subprocess
  → stdout/stderr/退出码解析
  → tool result 返回 root agent
```

不新增 daemon、HTTP server、MCP transport 或 durable workflow runtime。tool facade 内部仍保留 backend adapter 边界，但只在一次 tool call 生命周期内完成 worker 执行。

最小工具形态：

```text
run_worker(
  backend,
  profile,
  prompt,
  write_policy,
  data_classification,
  timeout_seconds
) -> ResultContract
```

需要并行时使用批量形式：

```text
run_workers(tasks[]) -> WorkerResult[]
```

第一版允许同步等待；只有真实任务证明需要长任务恢复时，才增加 `task_id`、后台状态和 `cancel`。这样可以先验证 root agent 通过工具自由选择 worker，避免为尚未出现的生命周期问题提前建设 MCP 基础设施。

tool facade 不做领域语义判断，不接受静默 backend fallback，不把 worker 失败包装成成功；它只负责参数门禁、子进程生命周期、结果解析和结构化返回。

### 14.21 先使用 OpenCode 原生 subagent，延后异构 CLI worker

重新评估后，tool facade 和多 CLI worker 不是当前必需项。若 OpenCode 原生 subagent 已经能完成任务拆分、并行只读调查、结果返回和基本失败处理，那么直接使用现有能力更简单，也更符合当前验证阶段。

当前优先路径调整为：

```text
OpenCode root agent
  → OpenCode 原生 subagent
  → 只读 research wave
  → join / evidence / ResultContract
```

只有出现以下具体缺口之一，才引入 tool facade 或 Codex/Claude CLI worker：

- 必须在同一 wave 中混用不同 provider/model；
- OpenCode 原生 subagent 无法满足所需的并行、超时或隔离；
- 需要独立 CLI 的 sandbox、session 或工具权限；
- 原生 subagent 的模型/成本/质量对当前任务明显不合适；
- 需要脱离 OpenCode 执行，支持其他宿主或无人值守批处理。

因此，前述 Worker Broker 保留为可逆候选，不进入当前 MVP 的实现范围。下一实验应直接验证 OpenCode 原生 subagent 是否能完成同一份 `research-wave-v0` 合同；验证通过则不新增 facade。

### 14.22 OpenCode 原生 subagent 实验结果

已在当前 OpenCode root 会话中直接并行调用三个项目原生 subagent，未经过外部 CLI adapter、MCP 或 Worker Broker：

```text
layout-owner          terra-explorer  completed
cross-facility-owner  terra-explorer  completed
evidence-entry        luna-extractor  completed
```

实验确认：

- root agent 可以直接调度项目 subagent；
- 三个 child 均完成并返回结构化调查结果；
- `terra-explorer` 和 `luna-extractor` 的只读边界生效，没有修改文件或运行命令；
- 结果包含 findings、claims、文件/行号 sources、unknowns、negative evidence，以及必要的 canonical conflict；
- domain side finding 可以独立标记为需要 reviewer，不影响识别 harness 执行状态；
- 当前 OpenCode 原生 subagent 已足够支撑公开只读 research-wave MVP。

这次实验没有验证 durable resume/cancel、长期后台任务或跨 provider worker 混用，但这些不是当前 MVP 的缺口。原生 subagent 路径已满足本阶段“拆分、并行、只读、返回和汇总”的核心假设。

因此当前结论收敛为：暂停 Worker Broker、MCP、Codex/Claude CLI adapter 和本地中转设计；先把现有 research-wave 结果接入统一 join/evidence 记录，再考虑任何异构 worker。
