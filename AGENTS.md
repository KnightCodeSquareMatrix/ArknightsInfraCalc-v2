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

先判断任务意图，再诊断改动形态：

| 意图 / 触发 | 必须使用 |
|---|---|
| bug、结果不对、CLI、数据、solver、局部排班 | `arknights-maintenance` |
| 新功能、恢复 TODO、新模式、接口或产品行为 | `arknights-feature` |
| 独立授权的架构、性能、工作流、技术债或 solver assurance | `arknights-quality` |
| 体系、跨设施、required anchor、scope、Team/Shift 绑定 | `arknights-system-audit` |
| build、test、CLI、benchmark、格式、结构或产物结论 | `arknights-evidence` |
| 练卡推荐**玩家**语义、名单、组合知识、输出口径、`--answer` 产品行为 | `arknights-feature` 或 `arknights-maintenance` + 先读 `docs/练卡推荐相关/README.md`，再完整读题面相关 canonical（新手/口径/组合/散件/审阅稿） |
| 练卡推荐**旧 v2 机器诊断**（`training_recommendations.json`、旧 now/conditional 过滤） | 仅兼容/对照；真源已归档见 `docs/ARCHIVE/superseded/练卡推荐规则_v2机器诊断合同.md`；不得用其覆盖 `docs/练卡推荐相关/` |
| 公孙长乐练卡知识/名单验收、与旧规则表对照 | `gongsun-training-review` |
| 建筑技能解锁阶段、`tier_0/tier_up`、`operator_instances` 设施绑定审计 | `arknights-building-skill-audit` |

debug / feature / quality Skill 是任务意图的 primary owner；`arknights-system-audit` 和 `arknights-evidence` 在触发时作为附加审计 / 证据协议，不接管任务目标与 scope。

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
- 共享工作区保持一个 writer。并行 writer 必须使用从明确 base SHA 建立的隔离 worktree。
- 非简单且存在独立调查、提取或审阅轴时，默认积极使用边界明确的只读 subagent；不为凑数量委派。主 Agent 必须检查真实文件、diff、日志和产物。
- 委派前按当前运行时选择真实可用的具名 profile；Codex 使用 `.codex/agents/`，OpenCode 使用 `.opencode/agents/`。任务名或提示词不能证明模型已切换；无法选择低成本 profile 时，不把机械提取批量回落到默认 Sol。
- 体系或 conformance-rebuild 写入前简述：领域不变量、首次违规位置、修复后的单一责任边界、要删除或改写的冲突路径。
- `shift_bind`、tag、priority、shortcut 只能消费各自语义，不能代替 required admission。
- 不为单一 operbox、room id、Shift 下标、fixture 或当前 top hit 写补偿特判。
- 每个写入单元只有一个主要意图；旁支发现默认 deferred。达到唯一 owner、冲突路径删除和相应证明后停止扩张。
- 新抽象应让真实重复语义、所有权或证明义务更清楚；“出现第二个用例”是审阅问题，不是禁止合法具名机制的硬规则。
- 修改候选集合、剪枝、分解、目标、top-K、Bake 或 cache 时，声明它是 hard constraint、safe reduction、heuristic、policy 还是 approximation；无法证明完整时不得声称全局最优。
- 所有用于结论的验证必须通过统一 evidence 工具留痕；未跑类别明确写“未跑”。
- 修改 Rust 后运行 `cargo fmt --all`；格式、测试和 CLI 同样通过 evidence 工具记录。
- Agent memory 只能记录可丢弃的检索提示与协作偏好；领域语义、当前实现、架构决策和活动任务必须落在版本化 canonical、ADR 或 change 文档中，memory 不得成为真源。
- 当前任务拥有 TODO、计划、change 或文档治理迁移时，生命周期语义唯一以 `docs/文档生命周期.md` 为准；`.agents/skills/_shared/CHANGE_LIFECYCLE.md` 只提供执行入口。完成时吸收 current facts、拆分开放项、自动归档并更新索引和引用。
- 代码变化不自动要求文档变化；只在行为语义、接口、持久决策或活动任务状态改变时更新最近的 owner。不得维护摘要复核传播或全仓 docs-impact 声明。
- 新增面向人类阅读的文档时优先使用清晰的中文文件名；协议固定名、工具约定、代码生成物或外部兼容路径可保留英文。不要仅为改名批量移动现有文档。
- 新建或恢复 `docs/TODO/` 任务时遵循 `docs/文档生命周期.md`；`docs/TODO/README.md` 只是生成的活动入口，不另行裁决状态。

### Workflow、checkpoint 与 memory 路由

Agent workflow 采用 [AI 编排与 Harness 讨论记录](docs/AI编排与Harness讨论记录.md) 第 8.3 节的已讨论模型，不在此重新发明一套状态机：

```text
bootstrap_context → classify_task → route_domain_owner → plan / plan-review
→ parallel research/review → join findings → select one implementation unit
→ single-writer executor → deterministic evidence verification
→ fresh-context review → final verification → checkpoint → memory capture → report
```

维护行为沿用 MiMo/Claude 分析中的分层边界：

- `checkpoint` 记录当前任务、已完成步骤、失败尝试、待处理结果和恢复所需上下文；长任务优先更新 checkpoint，不要求主 Agent 在 debug 中同时维护百科式 memory。
- `project memory` 只保存跨任务稳定有用的偏好、纠正、导航、进度和候选记忆；`task progress` 保存单任务进度；scratch notes 不晋升为事实。
- 领域语义、用户裁决、当前实现、架构决策和活动任务仍必须进入版本化 canonical、ADR 或 active-change；memory 不能覆盖它们，也不能成为第二真源。
- 重复出现且经验证的工作流才沉淀为 skill、command、agent 或 SOP；一次性经验保留为 checkpoint/候选 memory，不自动固化。
- 当前没有已验证的项目 memory backend 或独立 checkpoint writer 时，不创建隐式 `MEMORY.md` 或伪造自动晋升；把候选内容写入有生命周期的任务/决策文档，并明确标记其状态。

每个实际调查、决策或写入任务结束前，按上述 workflow 做一次维护收尾：检查最近 owner、checkpoint、候选 memory、deferred findings 和未验证风险；文档状态和归档严格服从 `docs/文档生命周期.md`。纯局部实现、测试补充和临时导航不强制新增文档。

## 4. 核心分层边界

- L1 interpreter 只认 `buff_id`，不认识干员名。
- L2 处理机制域；L3 shortcut 只结算实际组合，不负责体系选型或进编。
- Layout 主路径是 `build_plan -> execute_plan -> fill -> resolve`；排班不得绕过 Plan 修 admission。
- 生产搜索使用各域具名目标；多目标优先显式词典序，中枢或 global heuristic 必须具名。
- CLI / export 只编排调用和忠实输出，不写机制、公式或重新决策。

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
- 本轮形成独立单元时默认提交；同文件与用户改动无法可靠拆分时不强行提交。
- 发布到 quality-check 的提交标题必须使用中文；提交标题会作为质量网页中的摘要显示。
- `target/codex-runs/`、兼容日志和 `out/` 证据保留到交付但不提交。
- 最终说明 changed paths、文档影响、deferred findings、验证类别、未验证风险、commit 和剩余工作区状态。
