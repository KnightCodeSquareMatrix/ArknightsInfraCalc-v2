# Agent 知识库

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md；docs/INDEX.md
> 摘要：定义本地 Agent 检索入口、来源身份、历史过滤、回取与更新方式；领域事实仍由原 owner 裁决
> 代码范围：scripts/codex/query_project_knowledge.py；docs/agent-index/knowledge.json
> 代码锚点：scripts/codex/query_project_knowledge.py::main
> 核验基线：9f55a17；2026-11-01项目调查

## 使用与恢复

本地首版建设的目标、阶段、进度和压缩恢复见[完整计划](ARCHIVE/done/项目Agent知识库建设计划-20261101.md)。接续本任务先完整读计划，再核对 HEAD/status，从下一动作继续。一般问题从[INDEX](INDEX.md)、[检查与删减导航](检查与删减导航.md)或本地查询开始，读命中的 current owner 和源码后再作结论。

```bash
python3 scripts/codex/query_project_knowledge.py --query '三班心情复筛和重试'
python3 scripts/codex/query_project_knowledge.py --query '灵知戴菲恩自动中枢' --mode auto
python3 scripts/codex/query_project_knowledge.py --query 'flat_combinations' --kind code
python3 scripts/codex/query_project_knowledge.py --id <查询返回的ID> --parent --full
python3 scripts/codex/query_project_knowledge.py --query '心情预算' --history --output target/codex-runs/knowledge-query.json
```

返回原文片段与来源，不生成答案、不裁决冲突。ID 基于文件与标题路径或源码声明；重复标题带序号。行号用于当前定位；标题/符号重命名会使旧 ID 失效，不能凭 ID 沿用旧内容。

## 当前实现与保证

使用 Python 标准库，本地扫描治理 Markdown、Rust 声明和 route registry，按文件hash与解析脚本/依赖hash增量缓存分块与词项。精确子串、英文标识符、中文字符/二字词项加 BM25 排序，叠加人工别名用于真实问题导航；这是关键词检索，尚未实现 embedding、语义混合、LLM rerank 或完整 AST/调用图。

小片段命中后带原始元数据、父段定位、current owner、具名 code anchor 和 route。`--parent` 展开完整父问题段；范围与内容都带行号/hash。源码声明地图只用于导航，正则分块不能证明 trait、宏、实际调用或完整函数边界。

默认检索 current/canonical/current-reference 文档及当前源码；archive、proposal、checkpoint 和明确历史段必须 `--history` 才作为搜索命中。使用页内 `<!-- knowledge:historical -->` 标记历史标题段，标签不会因整页 canonical 而升级。历史命中仍返回 current owner；owner 已修改时即时读取原文，不能用排名挑选冲突赢家。未标记的混合历史正文仍可能召回，需回原文辨识；本轮治理不等于所有189页逐句审计。

结果标明 source role/status、section status、HEAD、SHA-256、dirty、核验基线和评分。dirty 仅表明工作树与提交不同，不证明获批。缓存只是加速资料，读取文件 hash 改变就重建该文件；每次查询实时读取来源并计算 hash，不引用过期摘要。

## 四种事实与精确资料入口

游戏技能原文去[固定知识库引用](知识库引用.md)，运行时映射去[输入与技能映射](模块/输入与技能映射.md)；产品支持去 canonical，当前实现去代码，某次结果去带 binary/input/commit 身份的证据。检索不会用 JSON、top hit 或助手总结替代裁决。

运行时干员/buff/Rule/shortcut 使用 `query_runtime_data.py` 的精确原始记录和 JSON Pointer；调查原话使用 `index_research_reports.py`；定时排班产物使用 `query_rotation_report.py`。后者当前要求 TimedReport，不支持 AutoReport；自动轮换读自身报告/调用链，不能仅凭 rotation 文件名推荐该脚本。

vault 按 routes 中固定 commit 读取，用 `--vault-root /home/knightcode/Projects/arknights-base-vault` 与 `--vault-path <routes中的路径>` 显式回取。不会跟随 vault 本地 HEAD，也不会扫描任意历史原话或外部账号数据；draft/未标状态保留原样，不升为 canonical。

## 更新与验证

文档治理继续采用[生命周期](文档生命周期.md)的一事实一 owner；检查导航和别名仅链接来源，不成为另一份业务真源。代码变化按已有触点更新 owner；调整导航时同步 `docs/agent-index/knowledge.json` 的标题/别名/关系，并运行本地 `--check` 和三项文档检查。源文件 hash 变化自动更新缓存，不需要提交生成缓存。

默认缓存 `target/codex-runs/knowledge-index.json`，查询产物只能放本仓 target/out，禁止覆盖索引缓存或任何输入/来源。预算输出复用 `_query_output.py`，超限标明 omissions 与 `--full/--output` 完整回取；`--check` 核对导航标题、owner 和路径，不证明来源正文正确。无匹配 exit1，输入/来源读取错误 exit2，成功 exit0。

真实问答、历史两侧、修改源文件后的缓存失效与截断回取用于验收，不添加源码测试。知识检索并非硬业务门禁，也不自动调用 solver、修复、联网、部署或运行测试。实际改善以真实任务减少误用/返工为准，首轮问题命中不证明所有 Agent 都会遵守。

## 范式依据与后续选择

2026-11-01 已读取的一手资料：[Aider repo map](https://aider.chat/docs/repomap.html)、[Anthropic context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)、[Contextual Retrieval](https://www.anthropic.com/engineering/contextual-retrieval)、[Azure hybrid search](https://learn.microsoft.com/en-us/azure/search/hybrid-search-overview)、[ParentDocumentRetriever 固定0.3.27源码](https://github.com/langchain-ai/langchain/blob/langchain%3D%3D0.3.27/libs/langchain/langchain/retrievers/parent_document_retriever.py)、[Tree-sitter](https://tree-sitter.github.io/tree-sitter/)、[LlamaIndex CodeSplitter](https://developers.llamaindex.ai/python/framework-api-reference/node_parsers/code/)、[Sourcegraph context](https://sourcegraph.com/docs/cody/core-concepts/context)、[GraphRAG](https://microsoft.github.io/graphrag/)。原文/URL/hash 快照保存在调查证据目录。

本项目采用来源身份、实时读取、repo map、关键词、父段和有限已证关系展开；不照搬作者实验的模型、chunk/K 或收益数字。只有真实问题显示词项召回不足才评估本地/外部 embedding；多客户端确需统一接口时再包装 MCP。暂无证据要求向量数据库或 LLM 全图摘要。

## 首轮验收范围（2026-11-01）

15条导航核验通过；12个点名真实问题均在前5条命中预期导航。另12个自由改述首轮在烘焙、静态散件、缺buff和真源冲突上暴露漏召回，已补项目词汇提示并复查四类；不是对任意中文问题的泛化保证。原文回取、默认/显式历史两侧、父段历史隔离、未知ID/无匹配、固定vault、1500字符预算及来源覆盖拒绝均走实际CLI。

对本任务新文档临时加入唯一词再恢复，变化与恢复各只重建1文件，恢复后词不再出现在原文；不是只比较缓存摘要。查询索引覆盖当时340个Markdown/Rust文件、约5488片段；正则地图有范围限制。关闭热查询重复写缓存后四次同问内部耗时635–675ms，中位660ms（外部墙钟732–771ms）；首次建索引约2–3s。该数字是检索CLI性能，不是排班速度；本轮未改solver或复测排班。

完整命令/问答/边界/计时在`target/codex-runs/project-rag-build-20261101/`，不提交运行缓存或源码测试。三项文档机械检查覆盖链接/生命周期/索引等，不证明所有正文最新。

## 独立盲验（2026-11-01）

三名独立只读 Agent 各自回答十个未参与首轮调词的问题，同时用 `INDEX + rg + owner 原文` 和知识入口定位证据。30 个问题的原始知识查询和5个修正后复查保留为 JSON；Agent 对传统入口、直接 owner、同文件和控制证据采用了不同计数口径，因此本轮不合并成单一准确率，也不把 Agent 自报计数当成可复算的结构化评测结果。

盲验确认知识入口把每题展示范围收敛为少量标题段，但不能代替跨文件判断。明确失败包括：缺 `SkillTable` 的自然语言问法漏掉直接导航，`dirty` 文档问题前五未命中生命周期 owner；不同 Agent 还观察到 generic 二三班和游戏机制/Plan 分层问法存在排序噪声，但主 Agent 用保存的当前查询复核时两题 top1 已正确，因此不登记为稳定失败。已为可由稳定项目词汇解决的问法补充导航提示；排序仍按原文和 BM25，不能为每道验收题制造答案卡。

默认 6000 字符终端预算可能只显示前五结果的一部分；需要评估排名或形成答案时使用 `--output` 或 `--full`，不能把终端截断当成完整 top-K。热缓存查询避免重复解析；`--no-cache` 每次重建全部文件只适合失效验证，不代表日常使用成本。独立报告与30题原始查询保存在 `target/codex-runs/project-rag-independent-20261101/`，不提交。盲验没有运行 solver、真实 Box 或当前 HEAD 排班性能测量。

本轮证据还不足以引入 embedding、向量数据库、完整 AST 或 MCP：已观察到的是少量项目词汇漏召回、中文宽词项排序和跨文件推理限制。修正后 `query_project_knowledge.py --check`、索引生成检查、文档清单检查和仓库事实检查全部通过，5个定向复查均成功返回。后续在真实任务中继续记录错误 owner、误用历史、用户纠正和返工；只有稳定的自然语言漏召回无法由 current owner 与有限导航解决时，再用同一批独立问题比较语义检索的净收益。
