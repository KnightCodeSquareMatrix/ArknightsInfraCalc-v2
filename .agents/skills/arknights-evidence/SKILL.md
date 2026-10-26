---
name: arknights-evidence
description: 大型重构、solver/search、体系/排班/导出、部署或需要长期审计的任务，用仓库脚本留下可复现的构建、测试、CLI、基准和产物证据。普通局部修复和文档改动直接跑命令即可。
---

# 证据留痕

命令细节见 `scripts/codex/README.md`。脚本只机械记录事实，不裁决领域语义或最优性。

- **包装命令**：`scripts/codex/run_evidence.sh --task <slug> --category <类别> --stem <名字> -- <命令>`。类别用 `build`、`targeted-test`、`full-suite`、`cli`、`performance`、`format`、`structure`。产物用重复的 `--artifact kind=path` 登记。每次重跑都保留，不覆盖失败记录。
- **full suite 对比**：`scripts/codex/compare_test_failures.py --baseline <旧日志> --current <新日志>`，比较的是失败名集合。
- **汇总**：`scripts/codex/render_evidence.py --manifest <manifest> --output <报告>`。缺的类别会标“未跑”。
- **文档检查**：`python3 scripts/codex/docs_inventory.py --check`、`python3 scripts/codex/check_repository_facts.py`、`python3 scripts/codex/generate_agent_index.py --check`。

证据要和风险匹配：

- 硬约束或资格：激活和拒绝两侧的边界，加最终可行性。
- 目标或 tie-break：各分量和等价最优的检查。
- 候选生成、剪枝、分解、Bake / cache 或性能：按 `docs/QUALITY_AND_AUDIT.md` 的求解保证章节，记录保证类别、差分 / oracle 或 fallback 证据和结果状态。改搜索空间的高风险改动，不能只靠一个 golden snapshot 通过。
- 排班 / 导出：见 `arknights-system-audit` 的证明要求。

没有确定性检查器的结论标“未验证”，不要把人工信心当成机器证明。
