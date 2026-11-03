# Codex 证据与任务边界工具

本目录只机械记录和检查事实，不裁决业务语义。领域正确性仍由用户裁决、当前领域 Markdown、reviewer 和主 Agent 判断。

## 定向查询与上下文预算

以下工具用于查询已有项目产物，避免先打印整份 JSON 或调查报告。RTK 继续负责通用命令输出压缩；这些脚本负责按本项目结构定位事实。

三个入口均输出 JSON：成功返回 0，无匹配或缺少显式预期文件返回 1，输入格式/读取错误返回 2。默认终端最多 6000 字符，`--max-chars N` 可调整（至少 1500）；超出时输出仍为合法 JSON，并标明省略位置、总量和展开方法。`--full` 展开完整查询结果，`--output <path>` 保存完整查询结果，终端仍受预算限制。禁止用 `--output` 覆盖输入或来源文件；完整结果留在 `target/codex-runs/` 或 `out/`，不提交。

### 项目知识与检查删减

```bash
python3 scripts/codex/query_project_knowledge.py --query '自动轮换中枢灵知' --mode auto
python3 scripts/codex/query_project_knowledge.py --query 'flat_combinations' --kind code
python3 scripts/codex/query_project_knowledge.py --id <返回ID> --parent --full
python3 scripts/codex/query_project_knowledge.py --check
```

本地关键词、Markdown父段与Rust声明导航，默认排除历史/提案，`--history`显式纳入；不是embedding语义检索。返回原文、owner、hash、HEAD、dirty、历史身份与完整回取，不裁决业务。详见[Agent知识库](../../docs/Agent知识库.md)、[检查与删减导航](../../docs/检查与删减导航.md)。源码声明是正则导航，不能证明AST或真实调用。缓存放target，hash改变按文件更新；文档/源码同时变化时查询会要求重读。

### 排班报告

```bash
python3 scripts/codex/query_rotation_report.py --report out/rotation.json --room control
python3 scripts/codex/query_rotation_report.py --report out/rotation.json --shift 2 --operator 灵知
python3 scripts/codex/query_rotation_report.py --report out/new-rotation.json --compare out/old-rotation.json
```

输入为定时模式 `plan --rotation-out` 或 `layout team-rotation --rotation-out` 保存的完整 Timed rotation JSON；当前不支持 AutoReport。profile 的 `rotation` 只有效率摘要，MAA 也采用不同结构，均会明确拒绝。房间和姓名精确匹配；`--shift N` 是一基班次编号，匹配保存的 `index == N-1`，不按数组位置取值，也不把 `plan_index` 当班次。

默认返回已有日产出、告警、班次时长、Plan 引用和设施效率；指定房间/干员后附命中房间的人员原字段和对应房间效率。来源包含路径、SHA-256，结果位置使用 JSON Pointer。合法省略的告警视为零；必需结构不符、重复索引/房间 ID、Plan 引用越界会报错，不能把缺数据解释为空结果。

`--compare` 比较完整、已解码的 rotation JSON，不与筛选参数组合；输出变化位置及 before/after，保留数组顺序、人员练度、动作、Plan 和时长，只忽略根 `/elapsed`。差异本身不表示错误，读取成功即返回 0；该比较不证明排班合法、导出等价或全局最优，不重新求解或结算机制。未知字段仍参与比较。

### 运行时数据

```bash
python3 scripts/codex/query_runtime_data.py --data-dir data --operator 杜林
python3 scripts/codex/query_runtime_data.py --buff-id 'control_tra_limit&spd2[000]'
python3 scripts/codex/query_runtime_data.py --rule trade_core
python3 scripts/codex/query_runtime_data.py --system ling_jie_karlan
python3 scripts/codex/query_runtime_data.py --shortcut gsl_docus_syracusa
```

只加载显式 catalog：operator instances、skill table、orchestration rules、legacy systems、producer rules、trade shortcuts/segments。不遍历 fixtures、Bake、feedback。默认数据目录来自脚本所属工作树，可用 `--data-dir` 指定另一工作树。

输出原始记录、来源字段和 JSON Pointer，关联精确相同的字符串值/键；干员查询另外附原始绑定的 skill 记录。不会用描述中的子串推断依赖，不合并 tier/stepwise，不重算效率或判断准入。`buff_ids_not_in_skill_table` 只是“该 ID 未收录”的事实，不能单独判断技能错误。缺 catalog、错误容器或重复记录 ID 明确报错。

### 上游基建技能缺失角色

下载固定提交的 building_data.json 与同提交 character_table.json 后运行：

```bash
python3 scripts/codex/report_missing_building_skills.py --building out/building-skills/<版本>/building_data.json --characters out/building-skills/<版本>/character_table.json
```

默认比较本工作树 data/skill_table.json 的精确 buff ID，逐角色输出未收录技能、设施、名称和 ID；另按 data/operator_instances.json 的姓名标记实例缺失。可用 --facility MANUFACTURE（可重复）筛选上游 roomType，--json 输出包含技能原文和解锁条件的完整 JSON。缺失是事实清单，不证明机制漏实现；不转换自定义 buff ID、不判定委托、不修改运行时数据。成功（含无差集）返回 0，读取或结构错误非零退出。产物保存在 out/，不提交。

加 --valuable 按用户 2026-11-03 的展示口径排除加工站、专精室（含中枢仅提升专精速度）及中枢以外纯心情加成，并附技能原文。心情互换、感知信息转化和中枢心情技能保留。非设施类排除只匹配 building_skill_exclusions.json 已复核的 buff ID、设施和原文 SHA-256；新 ID、原文或设施变化默认保留给用户，不证明技能已建模或可直接进池。纯效率、暖机、挂件和组合角色按现有代码及规范流程处理；其他类型只展示原文，不展开机制分析。

线索倾向也忽略：仅改变线索类型、概率或必得线索的技能排除；兼有速度加成的保留。会客室两两枚举求效率，组合技能复用现有流程。筛选不修改原始描述。脚本先核对角色表可获得的八职业干员集合与基建表 chars 完全一致，不一致非零退出；全部缺失实例在技能筛选前独立计算，始终输出 all_missing_instances、missing_operator_skills_in_scope（缺实例角色的全部范围内技能，不依赖技能表是否缺 ID）和名册 coverage，防止将筛选外角色误当作不存在。重复 JSON 键、重名干员、空技能关联或未知上游 buff 均非零退出，不静默覆盖或跳过。

### 调查报告与用户原话

```bash
python3 scripts/codex/index_research_reports.py --dir out/fact-sweep --expected r00.txt --expected r01.txt
python3 scripts/codex/index_research_reports.py --dir out/fact-sweep --source crates/infra-core/src/layout/assign.rs
python3 scripts/codex/index_research_reports.py --file out/fact-sweep/r00.txt --section 0 --full
python3 scripts/codex/index_research_reports.py --file out/four-gold.users.md --section 0001 --full
python3 scripts/codex/index_research_reports.py --file out/workflow.output --key rules-compile --section L1 --full
```

支持 Markdown 标题、行首 `[编号]` 的 TXT 结论，以及 `result[].key/report` 的 Workflow JSON。目录模式只读直接子文件中的 `.md/.txt/.json/.output`；JSON 必须符合 Workflow 格式，不猜任意 JSON 的含义。Markdown 忽略代码围栏内标题；段落到下一个同级/上级标题为止，普通标题 ID 为 `L<起始行>`，编号结论和用户消息段使用原编号。选段必须指定文件；同 ID 歧义报错，可用 `--key` 缩小 Workflow 范围。索引输出中的 ID 可直接用于选段。

索引区分“报告行号”与“引用源文件行号”；Workflow 使用 `/result/N/report` 和解码后报告的一基行号，不冒充 JSON 物理行号。用户消息保留角色、时间、message/part ID 及原文中存在的 synthetic/ignored/compaction flags；缺少 flags 不默认为 false。选段的 `text` 保存原始文字和换行，终端省略时用 `--full` 或 `--output` 取全。

`--expected` 只检查给定文件是否存在；未知标记只登记明确命中的原词。索引不能裁决报告正确性、矛盾、证据是否充分或哪条用户发言优先。正式复审仍需打开相关原文和代码。

### 批量验收进度

```bash
python3 scripts/assert_rotation_invariants.py --group all --output-dir target/codex-runs/task/batch --jobs 2
python3 scripts/assert_rotation_invariants.py --status target/codex-runs/task/batch
```

批量入口默认使用 `target/release/infra-cli`，构建也使用 release。指定 `--output-dir` 后，从构建阶段开始原子更新 `progress.json`，每盒实际完成时更新；终端进度默认至少间隔 15 秒，可用 `--progress-interval` 调整（0 为每盒输出）。`last_result` 是最后完成盒，不声称是当前运行盒；并行后盒先完成时进度也立即可见。

状态区分 `running/completed/failed` 与 `build/solve/compare/finished` 阶段，带完成数、各类失败数、更新时间、耗时和证据路径。比较结束后才记录最终退出码；构建、进程启动或产物写入异常记录批次错误并返回 2，不混入盒级错误类别。已有超时分类、最终 `summary.json` 和失败原因保留。

`--status` 只读取并验证快照，不构建、不求解；读取成功返回 0，即使记录的批次退出码非零。快照显示的是最后记录状态，不能证明进程仍存活；缺失/损坏返回 2。

## 统一命令包装器

```bash
scripts/codex/run_evidence.sh \
  --task issue-slug \
  --category cli \
  --stem mechanism-cases \
  --inputs 'data/REGRESSION_CASES.csv and data/UNIT_OUTPUT_ANCHORS.csv' \
  -- target/release/infra-cli verify --all
```

支持重复 `--artifact kind=path` 登记命令产物，支持 `--metadata task.json` 写入任务范围、旁支发现和 reviewer 声明。metadata 结构见 `task_metadata.example.json`。

证据默认写入：

```text
target/codex-runs/<task>/
  manifest.json
  commands/<run-id>/<category>-<stem>.log
  commands/<run-id>/<category>-<stem>.status
  reports/
```

包装器使用参数数组执行原命令，不执行 `eval`，并返回原命令 exit code。每个命令目录由原子 `mkdir` 占位，manifest 通过文件锁和原子替换更新，因此并发调用不会覆盖证据。

最终证据类别使用：`build`、`invariants`、`batch`、`cli`、`performance`。不新增、保留或运行源码测试。格式和结构检查可以使用 `format`、`structure`；渲染器会把它们列在“其他验证”。

## 批量失败与结果差分

```bash
python3 scripts/assert_rotation_invariants.py \
  --binary target/release/infra-cli --skip-build --group all \
  --output-dir target/codex-runs/task/batch \
  --compare-to target/codex-runs/baseline/batch
```

`summary.json` 分开记录不变量、求解、输入和超时失败，`progress.json` 用于读取进度。差分使用相同输入与 rotation，忽略耗时字段；失败和结果变化仍须按领域规则核对。旧 Cargo 测试日志可作为历史调查材料，不再执行或比较源码测试套件。

## 完成检查

```bash
python3 scripts/codex/docs_inventory.py --check
python3 scripts/codex/check_repository_facts.py
scripts/codex/check_task_scope.py --manifest target/codex-runs/<task>/manifest.json
scripts/codex/render_evidence.py \
  --manifest target/codex-runs/<task>/manifest.json \
  --output target/codex-runs/<task>/reports/evidence.md
```

`docs_inventory.py` 统一解析 [文档生命周期](../../docs/文档生命周期.md) 元数据，检查角色/状态/路径矩阵、唯一领域 owner、结构化页的代码锚点和关联链路。`generate_agent_index.py` 从 route registry、Rust 公共模块和 CLI dispatch 生成 Agent 路由与代码地图；`check_repository_facts.py` 再检查稳定 Markdown 链接、feedback ledger、CLI 命令地图和当前模块事实。生成器不把符号存在性冒充调用语义证明；跨层行为变更仍由作者声明触点并人工复核。

`check_task_scope.py` 检查实际 changed paths 是否属于 `change_scope`、带理由的 scope expansion 或证明路径；文档路径和其他消费者一样显式声明。显式 deferred 路径和 deferred side finding 被修改时硬失败。完成检查还要求 reviewer 登记最终不变量、精确 changed paths 和全部 expansion id。

`render_evidence.py` 会交叉核对 manifest、status、log 内的 exit code / PASS / FAIL，并检查所有登记产物存在。缺少的 build、终态断言与批量 Box、CLI、性能或 JSON 类别会明确渲染为“未跑”。

## 工具复核

本目录不保留源码测试。按修改触点执行实际入口：索引与文档检查、真实命令的证据包装、manifest 渲染、任务范围检查或实际产物定向查询。保留合法输入、非法输入及缺失/损坏证据两侧的检查，不以新测试框架替代这些入口。

`check_repository_facts.py` 同时检查已知源码测试形式不得重新出现：拒绝当前 `crates/` 下的 `#[test]`、`#[bench]`、`#[cfg(test)]`、Rust/Python 测试目录和 `scripts/` 下的 `test_*.py` 文件；不把合法的 `#[cfg(not(test))]` 当作源码测试，也不声称识别全部自定义测试形式。
