# Agent 协作指南

所有 agent 工具（Codex、Claude Code、opencode）共用本文件；`CLAUDE.md` 通过 `@AGENTS.md` 导入。根 `AGENTS.md` 是任务分类、真源顺序和项目硬门禁的唯一入口；这里只放跨任务的硬约束、工作偏好和入口，领域事实放在领域文档，这里只链接。

## 1. 项目

明日方舟基建机制、编制与排班求解器（Rust），也是未来一站式网站的核心引擎。bug、文档、新功能和质量建设会同时推进，不预设任务一定是小改，也不预设项目处于维护期或重构期。

- `infra-core`：机制解释、搜索、编排、排班和导出数据结构。
- `infra-cli`：命令入口、文件加载、输出格式化和回归壳；不写机制，不重新选型。
- `data/`：运行时载体，不裁决业务语义。
- `docs/`：领域语义、当前实现事实、决策与历史材料。

领域语义来自用户裁决、公孙长乐资料和 `arknights-base-vault` 知识库。

## 2. 协作方式

规则分三层。第一层不自行变通；第二层是用户的习惯，按任务裁剪；第三层先问再做。

### 2.1 硬约束

觉得某条不合理时，提出修改建议，不要自行绕过。

1. 用户和其他 writer 的既有改动不覆盖、不清理、不 stash、不混入本任务提交。开始和结束都看 `git status --short`。
2. 不做 destructive git：`reset --hard`、force push、amend、rebase、丢弃改动的 checkout、`clean`。只 stage 本任务的显式路径，不用 `git add .`。
3. 共享工作区只有一个 writer。并行写入时，各 writer 从同一 base SHA 建隔离 worktree，按不重叠的文件划分，由一个集成人逐个 `cherry-pick` 已验证的原子提交；不手工拼接冲突。
4. 分层边界：L1 interpreter 只认 `buff_id`，不认干员名；L3 shortcut 只结算实际组合，不负责选型或进编；CLI / export 只编排调用和忠实输出，不写机制、公式，也不重新决策。生产搜索使用各域具名目标，多目标优先显式词典序，中枢或 global heuristic 必须具名。
5. Layout 主路径是 `build_plan -> execute_plan -> fill -> resolve`，排班不绕过 Plan 修 admission。`shift_bind`、tag、priority、shortcut 只消费各自语义，不能代替 required admission。
6. 不为单一 operbox、room id、Shift 下标、fixture 或当前 top hit 写补偿特判。
7. 改候选集合、剪枝、分解、目标、top-K、Bake 或 cache 时，声明改动属于 hard constraint、safe reduction、heuristic、policy 还是 approximation；无法证明完整时，不声称全局最优。
8. 真源顺序：用户当前裁决 > 领域 canonical 文档 > 代码和生成 help（证明实现事实）> JSON、CSV、fixture、Bake、测试和旧输出。两份 canonical 冲突时，停止该处语义实现，请用户裁决；不要从代码或 top hit 猜。
9. 改 solver/search、Plan admission、Team/Shift、rotation、MAA/export、跨设施机制或大规模运行时数据时，完成前用 `arknights-invariant-gate` 做整体自检：在 Rust 最终 assignment 上跑 `--assert-invariants`，再用 `data/fixtures/box-bundles/` 里有代表性的集合和 high-owned 集合批量跑。

### 2.2 默认工作方式

这些是用户的习惯，按任务规模裁剪。偏离时在报告里写一行“通常 X，这次 Y，因为 Z”，然后继续。明显该变通的，直接做，事后报告。

- **先摸底再派**：派子代理之前，主代理先用几条命令摸清答案在哪、长什么样：数据存在哪个文件或哪张表、记录是什么格式、相关的东西分布在哪几个目录。摸底用汇总类命令（目录列表、计数、表结构、一两条截断的样本），不整文件读；亲眼看过原始样本才算摸清。摸底范围太大时，可以让子代理去取样本，但只收原始输出，不收结论。然后把问题切到每个子代理只回答一个具体事实：给出位置、要跑的命令或要找的字段、怎样算找到。写不出这样的问题，说明还没摸清，先接着摸，不要把“想办法查”交出去。一个子问题的答案不用参考别的子问题就能判断对错，才算切好；子问题之间有先后时，先派最没把握的那个，它的答案可能让其余计划作废。计数、取字段、查是否存在这类事，几条命令能做完的，主代理直接用脚本跑，不派。
- **调查**：主 agent 不独自长时间调查。摸底之后，按独立问题拆给 subagent 并行做，数量不设上限。默认用 `simple-query`（250k 上下文，足够应付大部分调查）；调查太大时拆成几个 `simple-query`，而不是换一个上下文更大的 agent。`readonly-investigator` 只留给一条前后强依赖、无法拆开、又超过 250k 的长链。委派简报写清问题、允许读取的路径、证据格式和输出上限；子代理不共享主会话上下文（Codex 下设 `fork_turns="none"`）。简报里要求子代理：
  - 实际情况和简报对不上时（文件不在、字段名不同、结果为空或全是同一个值），停下来原样报告看到了什么，不自己换方法接着做。
  - 每条结论附出处：跑过的命令和输出，或文件和行号。没找到就写“未找到”，并写明查过哪些地方。
  - 来源之间有分歧就如实写出，不自己调和；不给建议，判断归主代理。
- **子代理选型**：调查和写作都用项目自定义的子代理（`.claude/agents/`），不为“任务重要”改用更贵的模型或内置通用 agent。写文档、改索引、跑文档检查用 `doc-writer`；按行为说明实现一个代码单元、编译和跑点名的检查用 `code-writer`，提交由主代理做。现有类型缺某种权限时，照现有定义的模型加一个新类型补上权限，不换模型；确实需要更贵的模型时先问用户。
- **核对**：子代理的报告是线索，不是事实。会改变决定的结论，以及“没有”“不存在”这类否定结论，主代理一律回原文或原始数据核对；其余结论每份报告抽查一条。两份报告冲突时，回原始数据裁决，不采信最新的那份。
- **沟通**：汇报和提问讲行为和逻辑，必要时展开到具体例子，不用代码字段名代替解释。需要用户裁决时，直接说要他定什么，并用选项提问。
- **回答机制或结果问题**：先分清四层——游戏技能实际能触发什么、产品决定支持什么、当前求解器如何实现、这次输入实际得到什么。第一段用自然语言回答所问的那一层，再给证据。不要用实现里的准入条件否定游戏技能本身的低档协同，也不要用“允许进入候选”代替“为何最终被选中”。证据不足就说没查清。
- **问与做**：用户在提问、讨论或要诊断时，交付的是分析；用户要求修复时，做到验证并提交为止。报告时区分用户事实、用户推测、agent 推断和未知项。
- **写之前**：跨模块或改变行为的写入，先用行为语言说明：输入或前置状态、候选如何进入或被拒、状态改变的顺序、输出及其消费者、哪些保持不变、怎么验证。用户指出说明有误时，先修正说明再写。局部修复不需要这一步。
- **修在哪**：改最早放行错误状态的那个 owner，不在下游补偿。owner 已经能表达规则时直接局部修，不要求先做架构准备。
- **体系或结构性改写**（现有模型表达不了已确认的不变量，或多个阶段在重复兜底）：先简述领域不变量、首次违规的位置、修复后唯一的责任边界，以及要删掉的冲突路径。完成时旧的过渡路径和兼容分支一并删掉。
- **范围**：一个写入单元只有一个主要意图，旁支发现记下来延后处理。修 bug 时发现的"代码可以更干净"另起任务，不借 bug 扩大范围；改变能力、可行行为、policy 或输出口径的，按新功能对待。没有用户恢复授权时，不推进无关的历史 TODO、Mower 动态换班或全基建连续心情最优化。
- **验证**：按风险匹配。实现闭环后跑一轮完整验证包；失败时先改完，再整轮重跑，不拿反复编译代替调查。选人、排班、导出类的大改动靠 §2.1-9 的终态断言加批量 box，不新增源码测试；单房间计算这类局部修复可以补最小复现测试，改候选或搜索时同时测进入和被排除两侧。只改单个房间的计算或只改文档时，可以不跑 §2.1-9 的整体自检，但要在报告里说明原因。复现入口不清时查 `docs/MAINTENANCE_MODE.md` 对应章节。
- **复审**：中高风险的选人、排班、导出或跨模块改动，整个功能完成后用 `arknights-intent-review`。复审只查三件事：是否照用户的想法改、实现是否干净、耦合代码依赖的条件是否还成立。准绳是用户原话和改动前的裁决，不是本次刚改的文档。主 agent 不自己复审，也不重复验收；每条发现必须落到已修复、有证据驳回或交给用户之一。
- **提交**：授权的修复验证通过后，主动做原子提交。标题用中文，因为会显示在质量网页上。提交前看 `git status --short`、`git diff`、`git log --oneline -10`。
- **文档**：改动涉及入口、状态写入、分支或 fallback、错误、输出、机制映射或保证等级时，在同一个变更里更新对应的 owner 文档并跑文档检查；纯内部重构写一句“已复核，无需修改”。新的人读文档用中文文件名。
- **长任务**：用户授权持续推进时，一个回合结束不等于任务结束。把已核实的事实、剩余范围和下一步写进对应的 active-change 文档，不依赖用户答案的工作继续做。要交接给后续 agent 时，按用户要求切分支，并写明工作树里哪些改动属于谁。用户授权无人值守修复时，按 `docs/QUALITY_AND_AUDIT.md` §2.3 处理暂停。
- **交付说明**：写清改了哪些路径、文档影响、延后的发现、跑过哪类验证、哪些风险没有验证、commit，以及工作区剩余状态。

### 2.3 先问再做

- 改变产品或机制语义，或改变用户可见的输出口径。
- 降低保证等级，或把 heuristic 当成完整搜索。
- 外部或不可逆的副作用：push、部署、发布、删数据、动其他 writer 的工作区。
- 两份 canonical 冲突，或证据指向的方案要推翻用户已有的裁决。

提问时带上发现、2–3 个选项及各自后果、你的推荐；有结构化提问 UI 就用它。提问只阻塞依赖答案的那部分，其余工作照常推进。

## 3. 环境与命令

- 测试、批量 box 求解和 `--assert-invariants` 验收一律用 release 构建：`cargo test --release`、`cargo build --release -p infra-cli`、`target/release/infra-cli`。debug 下全量库测试约 41 分钟。批量脚本默认指向 `target/debug/infra-cli` 时，用 `--binary target/release/infra-cli` 覆盖。
- 修改 Rust 后运行 `cargo fmt --all`。
- 需要长期留痕的证据（命令包装、失败集合对比、汇总报告）：见 `scripts/codex/README.md`。
- 文档检查：`python3 scripts/codex/generate_agent_index.py --check`、`python3 scripts/codex/docs_inventory.py --check`、`python3 scripts/codex/check_repository_facts.py`。
- `target/codex-runs/`、兼容日志和 `out/` 里的证据保留到交付，但不提交。

### 定向查询与上下文节省

查询已有产物时，优先用下列入口定位所需事实，再按需展开原文；RTK 负责通用命令输出压缩。详细参数与格式见 [工具说明](scripts/codex/README.md#定向查询与上下文预算)，不必每次通读。

| 需要查什么 | 入口 |
|---|---|
| 排班人员、班次、房间、产出及两份结果的变化 | `python3 scripts/codex/query_rotation_report.py --report <rotation.json>`；按需加 `--shift N`、`--room <id>`、`--operator <姓名>`，或单独用 `--compare <旧报告>` |
| 干员、buff、Rule、体系、shortcut 的原始记录与引用 | `python3 scripts/codex/query_runtime_data.py`，选择 `--operator`、`--buff-id`、`--rule`、`--system`、`--shortcut`；另一工作树用 `--data-dir` 指定 |
| 多代理报告目录、引用位置或用户原话选段 | `python3 scripts/codex/index_research_reports.py --dir <目录>` 或 `--file <报告>`；按需加 `--source <引用路径>`、`--section <编号>`，Workflow 可加 `--key` |
| 批量验收进度 | `python3 scripts/assert_rotation_invariants.py --status <运行目录>`；批量运行用 `--output-dir` 生成 `progress.json`，最终错误读 `summary.json` |

三个查询入口默认最多输出 6000 字符；省略内容用 `--full` 或 `--output <完整结果路径>` 回取，不把截断当作完整证据。排班查询只接受完整 rotation 产物，`--shift` 为保存的 `index+1`；profile/MAA 不冒充完整排班。数据查询不解释机制，报告索引不替代原文复审，进度快照不证明进程仍存活。命令留痕与失败集合比较继续复用 `run_evidence.sh`、`render_evidence.py`、`compare_test_failures.py`。

## 4. 从哪里读

- 按症状找文档：[`docs/INDEX.md`](docs/INDEX.md)。代码 owner 和命令：[`docs/PROJECT_MAP.md`](docs/PROJECT_MAP.md) 的相关段，不需要通读。
- 文档角色、生命周期与归档：[`docs/文档生命周期.md`](docs/文档生命周期.md)。
- 保证等级、风险分层验证、无人值守：[`docs/QUALITY_AND_AUDIT.md`](docs/QUALITY_AND_AUDIT.md)。
- 游戏机制不在本仓复制：先读 [`docs/知识库引用.md`](docs/知识库引用.md)，再进入固定版本 `arknights-base-vault` 的对应章节。
- 触及下列主题时，先完整读对应文档：

| 主题 | 先读 |
|---|---|
| 控制中枢选位、效果账本 | `docs/CONTROL_CENTER_ASSIGNMENT.md`；进行中的重构：`docs/TODO/中枢效果账本与顺序选位重构.md` |
| 能力影子候选、跨设施共享归属 | `docs/TODO/能力影子候选设计.md`（proposal；当前适用域仅夜烟）。它与挂件 receiver / SupportPool 是两个问题 |
| 技能档位 `tier_0` / `tier_up` | `docs/模块/输入与技能映射.md` |
| 练卡推荐 | `docs/练卡推荐相关/README.md`。旧 v2 机器诊断合同已归档，只用于兼容对照，不能覆盖该目录 |

## 5. Skills

普通 bug、新功能和质量改进不需要专门的 skill，按本文件做即可。下面这些在碰到对应内容时加载：

| 碰到 | 用 |
|---|---|
| 技能、效率、无人机、电力、心情、订单、单位产出或运行时数据映射 | `arknights-mechanics-contract` |
| 体系、跨设施、required anchor、scope、Team/Shift 绑定、排班或导出 | `arknights-system-audit` |
| 大型 solver/search、Plan、rotation、MAA/export、跨设施或机制变更的整体自检 | `arknights-invariant-gate` |
| 中高风险改动完成后的复审 | `arknights-intent-review` |
| 拉取线上反馈 / 求解失败证据、按 CLI 指纹查失败 | `arknights-feedback-sync` |
| 部署求解器 / 批量质检 API | `arknights-solver-deploy`、`arknights-batch-quality-api` |
| 建筑技能解锁阶段、`operator_instances` 设施绑定审计 | `arknights-building-skill-audit` |

## 6. 这份文件怎么改

- 每一条都要通过一个检验：删掉它，agent 会不会犯错？不会就删。
- 新增硬约束要附来源：引发它的事故 commit，或用户裁决。agent 提出的规则先作为建议交给用户，确认后再写入。
- Agent memory 只记可丢弃的检索提示和协作偏好。领域语义、用户裁决、当前实现、架构决策和活动任务，都写进版本化的 canonical、ADR 或 active-change 文档。
