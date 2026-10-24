# Agent 引导

## 1. 项目定位

这是明日方舟基建机制、编制与排班求解器，也是未来一站式网站的核心引擎。项目持续同时处理 bug、文档一致性重建、新功能和独立质量建设；不要预设所有任务都应小改，也不要预设项目默认处于维护期或重构期。

- `infra-core`：机制解释、搜索、编排、排班和导出数据结构。
- `infra-cli`：命令入口、文件加载、输出格式化和回归壳；不写机制或重新选型。
- `data/`：运行时载体，不裁决业务语义。
- `docs/`：canonical 领域语义、当前实现事实、工作流与历史材料。

未经用户恢复，不自动推进无关历史 TODO、Mower 动态换班或全基建连续心情最优化。

## 2. 真源与任务路由

真源优先级：

1. 用户当前明确裁决。
2. 对应领域 canonical Markdown。
3. 代码和生成 help 证明当前实现事实。
4. JSON、CSV、fixture、Bake、测试和旧输出只承载或核对实现。

两个当前领域 Markdown 冲突时停止语义实现，请用户裁决并先统一文档；不得从代码或 top hit 猜测。

回答机制或结果问题时，先分清四层：游戏技能实际能触发什么、产品决定支持什么、当前求解器如何实现、这次输入实际得到什么。第一段用自然语言回答用户所问的那一层，再给关键证据。不要用实现中的体系准入条件否定游戏技能的低档协同，也不要用“允许进入候选”代替“为何最终选中”；证据不足时明确尚未查清并继续定位。

先判断任务意图，再诊断改动形态：

| 意图 / 触发 | 必须使用 |
|---|---|
| bug、结果不对、CLI、数据、solver、局部排班 | `arknights-maintenance` |
| 技能、效率、无人机、电力、心情、订单、单位产出或运行时数据映射 | `arknights-maintenance` + `arknights-mechanics-contract` |
| 新功能、恢复 TODO、新模式、接口或产品行为 | `arknights-feature` |
| 独立授权的架构、性能、工作流、技术债或 solver assurance | `arknights-quality` |
| 大型 solver/search、Plan、rotation、MAA/export、跨设施或机制变更后的最终状态门禁 | `arknights-invariant-gate` + `arknights-evidence` |
| 体系、跨设施、required anchor、scope、Team/Shift 绑定 | `arknights-system-audit` |
| build、test、CLI、benchmark、格式、结构或产物结论 | `arknights-evidence` |
| 练卡推荐**玩家**语义、名单、组合知识、输出口径、`--answer` 产品行为 | `arknights-feature` 或 `arknights-maintenance` + 先读 `docs/练卡推荐相关/README.md`，再完整读题面相关 canonical（新手/口径/组合/散件/审阅稿） |
| 练卡推荐**旧 v2 机器诊断**（`training_recommendations.json`、旧 now/conditional 过滤） | 仅兼容/对照；真源已归档见 `docs/ARCHIVE/superseded/练卡推荐规则_v2机器诊断合同.md`；不得用其覆盖 `docs/练卡推荐相关/` |
| 公孙长乐练卡知识/名单验收、与旧规则表对照 | `gongsun-training-review` |
| 建筑技能解锁阶段、`tier_0/tier_up`、`operator_instances` 设施绑定审计 | `arknights-building-skill-audit` |

debug / feature / quality Skill 是任务意图的 primary owner；`arknights-system-audit` 和 `arknights-evidence` 在触发时作为附加审计 / 证据协议，不接管任务目标与 scope。

机制与数据口径变更由 `arknights-mechanics-contract` 提供附加契约；候选集合、剪枝、目标、Bake 或 cache 的 medium/high-risk 改动应调用 `solver-guarantee-reviewer`；Team/Shift、心情、休息、绑定或 MAA 改动应调用 `rotation-maa-reviewer`。它们都是只读领域复核，不接管写入或最终裁决。

`local` 表示正确 owner 已存在、只修 owner 内部；`conformance-rebuild` 表示现有模型无法表达已确认不变量，或多个阶段在重复兜底。它们是诊断结果，不是与 debug/feature/quality 并列的用户意图。

`formal-audit` 只在用户要求严格逐项审计或 canonical 文档冲突时启用。只读 research 可按不同调查轴使用 explorer/extractor，不自行获得修改授权。

领域入口未知时查 `docs/INDEX.md`；代码 owner 或命令事实未知时查 `docs/PROJECT_MAP.md` 的相关段，不默认通读。选定的 Skill 和 canonical 领域文档必须完整读取。

根 `AGENTS.md` 是任务分类、真源顺序和项目硬门禁的唯一入口；`docs/INDEX.md` 只负责定位文档与后续阅读，不另行定义 Skill 触发词或真源优先级。

### 影子候选设计入口

凡讨论或实现“能力影子候选”、跨设施/跨产物共享干员归属、同房组合候选延迟占用或冲突分支重算，必须先完整读取
`docs/TODO/能力影子候选设计.md`。该方案把 capability shadow 与 physical operator
分离，在冲突闭包内比较候选后才提交真实 assignment；它与挂件 receiver、SupportPool、SupportClaim
是两个独立问题，不得混为同一抽象或验收项。该文档当前是 `active-change / proposal`，不代表代码已经实现。

## 3. 所有任务通用硬门禁

- 未授权修改时只读；诊断不自动等于实现。区分用户事实、用户推测、Agent 推断和关键未知项。
- 开始写入前检查工作区；用户既有改动默认属于用户，不覆盖、不清理、不混入本任务。
- 共享工作区保持一个 writer。并行任务优先拆分为只读调查；确需并行写入时，按不重叠的文件 owner 划分，所有 writer 从同一明确 base SHA 建立隔离 worktree，并由一个集成人负责顺序提交。
- 并行 writer 不手动 merge 文件内容：集成人逐个 `cherry-pick` 已验证的原子提交。若两个任务必须改同一文件，改为串行执行或由单一 writer 集成；不得并行编辑后依赖人工冲突拼接。
- 非简单任务（包括看似局部的 bug、编译失败或结果异常）必须先拆出独立调查轴，再选定写入单元：存在两个以上独立轴时，写入前按轴分别委派相应数量的只读 subagent；有三个以上独立问题时，第一轮至少启动三个 investigator。每份委派简报限定问题、允许来源、证据格式和停止条件；主 Agent 汇总去重、核对引用并收敛 findings 后，才交给单一 writer 实施。不得以“先修眼前问题”、主线程重复调查、单个宽泛委派或已有修复设想替代并行取证；编译错误等近端故障应作为一条调查轴，不构成跳过其他轴的理由。已有代理结论时先复用并核对原始证据，只为明确缺口补派。
- 子代理始终使用独立上下文：Codex collaboration 委派显式设置 `fork_turns="none"`，提供只含目标、边界、必要来源与验证问题的自包含简报，不复制主会话全文或其他代理结论。共享文件系统不等于共享对话上下文。
- 对边界已明确的阶段性分析、计划、当前代码/diff 或 evidence packet，优先用 `stage-reviewer` 做快速对抗初审；它只能裁决当前阶段是否继续，不能替代用户语义裁决或适用的领域 Sol 最终审阅。
- 用户明确授权无人值守修复时，按 `docs/QUALITY_AND_AUDIT.md` §2.3 的固定 `PAUSE_REASON`、冻结证据包和重试上限自动推进；未触发该枚举不得因一般不确定性暂停，也不得自行扩大业务语义、保证等级或外部副作用范围。
- 体系或 conformance-rebuild 写入前简述：领域不变量、首次违规位置、修复后的单一责任边界、要删除或改写的冲突路径。
- `shift_bind`、tag、priority、shortcut 只能消费各自语义，不能代替 required admission。
- 不为单一 operbox、room id、Shift 下标、fixture 或当前 top hit 写补偿特判。
- 每个写入单元只有一个主要意图；旁支发现默认 deferred。达到唯一 owner、冲突路径删除和相应证明后停止扩张。
- 新抽象应让真实重复语义、所有权或证明义务更清楚；“出现第二个用例”是审阅问题，不是禁止合法具名机制的硬规则。
- 修改候选集合、剪枝、分解、目标、top-K、Bake 或 cache 时，声明它是 hard constraint、safe reduction、heuristic、policy 还是 approximation；无法证明完整时不得声称全局最优。
- 对 solver/search、Plan admission、Team/Shift、rotation、MAA/export、跨设施机制或大规模运行时数据变更，完成前必须使用 `arknights-invariant-gate`：在 Rust 最终 `assignment` 上注册 `--assert-invariants` 检查，并用 Python 对 `data/fixtures/box-bundles/` 的代表性与 high-owned 集合批量调用该 gate。Python 不得解析文本/MAA 或重写机制语义；它只报告 Rust gate、求解无解、输入错误和超时。实际 assertion diff 必须经 Sol 复审至 `PASS`。
- 验证按改动风险选择方式：大型重构、solver/search、体系/排班/export、部署和需要长期审计的任务，使用 evidence 工具留痕；普通局部修复和文档调整可直接运行必要命令，并在最终说明结果。
- 修改 Rust 后运行 `cargo fmt --all`；格式、测试和 CLI 是否通过 evidence 工具记录由任务风险决定。
- 运行测试、批量 box 求解和 `--assert-invariants` 验收一律使用 release 构建（`cargo test --release`、`cargo build --release -p infra-cli`、`target/release/infra-cli`）。debug 构建下端到端排班用例极慢（全量库测试约 41 分钟），不得作为验收手段。批量脚本默认二进制若指向 `target/debug/infra-cli`，运行时用 `--binary target/release/infra-cli` 覆盖。
- Agent memory 只能记录可丢弃的检索提示与协作偏好；领域语义、当前实现、架构决策和活动任务必须落在版本化 canonical、ADR 或 change 文档中，memory 不得成为真源。
- 当前任务拥有 TODO、计划、change 或文档治理迁移时，生命周期语义唯一以 `docs/文档生命周期.md` 为准；`.agents/skills/_shared/CHANGE_LIFECYCLE.md` 只提供执行入口。完成时吸收 current facts、拆分开放项、自动归档并更新索引和引用。
- 代码变化不要求全仓文档传播，但入口、状态写入、分支/fallback、错误、输出、机制映射和保证等级变化必须按 [`docs/文档生命周期.md`](docs/文档生命周期.md) 复核对应模块页、调用链页、接口页和 route registry；纯内部重构若锚点与行为仍成立，明确记录“已复核无需正文修改”。作者声明行为触点，不能只依赖路径映射；不得维护无边界的全仓摘要复核传播。
- 新增面向人类阅读的文档时优先使用清晰的中文文件名；协议固定名、工具约定、代码生成物或外部兼容路径可保留英文。不要仅为改名批量移动现有文档。
- 新建或恢复 `docs/TODO/` 任务时遵循 `docs/文档生命周期.md`；`docs/TODO/README.md` 只是生成的活动入口，不另行裁决状态。

### Workflow、checkpoint 与 memory 路由

Agent workflow 采用 [AI 编排与 Harness 讨论记录](docs/AI编排与Harness讨论记录.md) 第 8.3 节的已讨论模型，不在此重新发明一套状态机：

```text
bootstrap_context → classify_task → route_domain_owner → plan / plan-review
→ parallel research/review → join findings → select one implementation unit
→ single-writer executor → risk-matched verification
→ fresh-context review → final verification → checkpoint → memory capture → report
```

维护行为沿用 MiMo/Claude 分析中的分层边界：

- `checkpoint` 记录当前任务、已完成步骤、失败尝试、待处理结果和恢复所需上下文；长任务优先更新 checkpoint，不要求主 Agent 在 debug 中同时维护百科式 memory。
- `project memory` 只保存跨任务稳定有用的偏好、纠正、导航、进度和候选记忆；`task progress` 保存单任务进度；scratch notes 不晋升为事实。
- 领域语义、用户裁决、当前实现、架构决策和活动任务仍必须进入版本化 canonical、ADR 或 active-change；memory 不能覆盖它们，也不能成为第二真源。
- 重复出现且经验证的工作流才沉淀为 skill、command、agent 或 SOP；一次性经验保留为 checkpoint/候选 memory，不自动固化。
- 当前没有已验证的项目 memory backend 或独立 checkpoint writer 时，不创建隐式 `MEMORY.md` 或伪造自动晋升；把候选内容写入有生命周期的任务/决策文档，并明确标记其状态。

每个实际调查、决策或写入任务结束前，按上述 workflow 做一次维护收尾：检查最近 owner、checkpoint、候选 memory、deferred findings 和未验证风险；文档状态和归档严格服从 `docs/文档生命周期.md`。纯局部实现、测试补充和临时导航不强制新增文档。

### 长任务的行为说明、提问与续做

- 用户授权持续推进或无人值守的长任务时，单个对话回合结束不等于任务完成。结束一回合前要把已核实事实、未完成范围、风险匹配验证和下一个具体工作单元写入 active-change/checkpoint；若仍有独立且获授权的工作，不得只留一段进度总结后停住。
- 用户给出局部决策后，立即更新决策文档；只解锁依赖该答案的部分。未决项不得成为其他独立工作、调查或文档修正的停止条件。
- 每个源码写入单元开始前，先向用户说明**行为**：输入/前置状态、候选如何进入或被拒绝、状态改变顺序、输出及其消费者、行为保持边界、允许变化和验证场景。以人能核对的逻辑描述，不用函数名、文件拆分或“提取 helper”代替行为说明。若用户指出解释有误，先修订该单元行为说明，再继续相关写入。
- 需要用户在互斥的产品行为之间选择时，若当前 Codex 模式提供结构化提问 UI，应使用该功能，用简短、互斥且描述结果的选项提问；不要要求用户判断 JSON 省略/空数组等实现细节。提问只阻塞直接依赖答案的单元，其余工作照常继续。
- 若当前模式不提供结构化提问 UI，不要用长段手写多选题冒充按钮式交互；只针对会改变行为边界的关键问题，提出一句自然语言问题并明确示例后果，同时继续独立工作。不要因“已经问了”而结束整个任务。
- 子代理调查提示必须限定允许读取的路径、待回答的具体问题、最大输出长度、证据格式和禁止扩展范围；让它报告实际行为与未证实项，不让它自行发散成架构方案。主代理检查引用的实际文件和证据后，更新 checkpoint。
- 用户要求交给后续 Agent 时，创建/切换到用户要求的新分支，更新活动计划中的裁决、当前状态、验证证据、脏工作树归属与明确下一步；禁止清理或提交无法归属的既有共享改动。

## 4. 核心分层边界

- L1 interpreter 只认 `buff_id`，不认识干员名。
- L2 处理机制域；L3 shortcut 只结算实际组合，不负责体系选型或进编。
- Layout 主路径是 `build_plan -> execute_plan -> fill -> resolve`；排班不得绕过 Plan 修 admission。
- 生产搜索使用各域具名目标；多目标优先显式词典序，中枢或 global heuristic 必须具名。
- CLI / export 只编排调用和忠实输出，不写机制、公式或重新决策。

### 技能档位与精英化映射

- 干员设施技能的运行时状态统一为 `tier_0` 与 `tier_up` 两档。
- `tier_0` 对应上游 phase 0；`tier_up` 按干员星级映射到真实解锁阶段：1–2 星为 30 级，3–4 星为精一，5–6 星为精二；已精二干员统一属于 `tier_up`。
- 机制、倍率和候选逻辑以 `PromotionTier` 及 `operator_instances.json` 解析后的 `buff_id` 为输入；精英化阶段用于得到对应运行时档位。

### 控制中枢效果账本与顺序选位

- 已授权的中枢 conformance rebuild 唯一执行计划见
  `docs/TODO/中枢效果账本与顺序选位重构.md`；开始相关写入前必须完整读取该计划和
  `docs/CONTROL_CENTER_ASSIGNMENT.md`。
- 中枢同种效果取最高由 atom 级结构化 `EffectKey` 和显式 aggregation 决定；不得按干员名、
  `buff_id` 前缀、技能名或数值单位推断同类效果。冻结 ledger 是合并、trace、候选、dependency
  和投影视图的唯一 owner。
- 中枢跨域不使用综合评分函数。按 Plan hard anchor/原子体系、贸易全局区、制造全局区、训练室、
  办公室、会客室、心情去重、普通补位的既定顺序选位；后序不得替换前序。条件只读取当前已选
  成员和当前候选，不引入 witness package、回溯或额外组合搜索。
- 控制中枢 CSV inventory 必须 fail-closed：每行/tier 显式标记 supported、unsupported、ignored 或
  delegated。公式不明确的概率/倾向机制不得进入候选、补位、dependency 或 `shift_bind`。
- `PickOne` / `OptionalFixed` 只由 Plan 编译解析；execute 和 rotation 只能消费 resolved facts。
  `min_count` 保持候选组 hard constraint。心情实际结算和菲亚、办公室独立轮换不在本轮扩张。

## 5. Git 与交付

- 开始和结束都运行 `git status --short`。
- 只 stage 本任务文件，使用显式路径；禁止 `git add .`。
- 不自动 amend、rebase、reset、checkout、清理未跟踪文件或执行其他 destructive Git 操作。
- 每个授权修复必须完成“实现、风险匹配验证、复核、原子提交”的闭环；除非用户明确要求不提交，修复停止前主动提交，不得把“用户未明确要求提交”当作不提交理由。
- 提交前必须检查 `git status --short`、`git diff`、`git log --oneline -10`，仅 stage 本任务的显式路径；提交后再次检查状态。当前 writer 的 worktree 必须干净；共享主工作区如仍有其他 writer 的改动，必须逐项归属并说明，禁止为求干净而清理、stash、revert 或提交他人改动。
- 发布到 quality-check 的提交标题必须使用中文；提交标题会作为质量网页中的摘要显示。
- `target/codex-runs/`、兼容日志和 `out/` 证据保留到交付但不提交。
- 最终说明 changed paths、文档影响、deferred findings、验证类别、未验证风险、commit 和剩余工作区状态。

### 办公室 fallback 池提醒

- 办公室选人由办公室模块按自身效率规则决定，候选池必须来自主路径当前 assignment 的统一裁剪结果。
- 未来允许增加专门的办公室 fallback 池；当前 fallback 池为空实现，未启用。
- fallback 池不得通过静默回读全 roster 扩大候选集合；池子耗尽或状态不一致继续 fail-fast。

### Agent 文档定位入口

- 文档首读是 [`docs/INDEX.md`](docs/INDEX.md)，按症状进入生成的 Agent 路由；任务分类和 Skill 仍只看本文。
- 完整调用链必须读取 `docs/调用链/`，模块责任和代码锚点读取 `docs/模块/`，协议与字段读取 `docs/接口/`。
- 游戏机制不在本仓复制；先读 [`docs/知识库引用.md`](docs/知识库引用.md)，再进入固定版本的 `arknights-base-vault` 具体章节。知识库草稿和旧 solver 镜像不能升级为当前事实。
- 修改调用入口、状态传播、轮换/导出、机制映射或搜索保证时，同一变更复核关联文档并运行：`python3 scripts/codex/generate_agent_index.py --check`、`python3 scripts/codex/docs_inventory.py --check`、`python3 scripts/codex/check_repository_facts.py`。
