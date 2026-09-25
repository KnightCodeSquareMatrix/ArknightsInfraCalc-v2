# Agent 文档路由索引

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md
> 摘要：按症状、入口和输出把 Agent 路由到完整调用链、模块 owner、机制引用和验证入口

这里是机器可读路由的说明页。`routes.json` 是唯一手写注册表，`docs/生成索引/` 下的 Markdown 是生成结果，不手工编辑。

## 路由记录格式

每条 route 必须给出：

- `id`：稳定链路 ID，例如 `CLI.PLAN`、`SOLVER.TRADE_ROOM`；
- `priority` 与 `when`：症状重叠时的优先级和判别规则；
- `symptoms`：用户或日志中可能出现的症状；
- `entrypoints`：CLI、Worker 或 public Rust 入口；
- `chain_doc`：描述阶段顺序、状态读写、分支和失败传播的调用链页；
- `module_docs`：负责实现的模块页；
- `mechanics`：知识库具体目录或章节；
- `vault_refs`：隔壁知识库在固定 commit 下的具体原文路径；
- `verification`：可复现命令或最终状态 gate。

索引只帮助定位，不替代正文。生成器能证明路径存在和注册表覆盖，不能证明自然语言描述仍然符合分支语义；行为变更仍需人工检查调用链页。

## 查询顺序

```text
症状/关键词 → docs/生成索引/Agent路由.md → chain_doc → module_docs
→ docs/知识库引用.md + vault_refs 指向的机制原文 → verification
```

代码锚点以源码为准，当前文件和符号位置由 `docs/生成索引/代码与文档地图.md` 提供快速入口。
