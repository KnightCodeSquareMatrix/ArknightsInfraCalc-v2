# 历史建设期 TODO

> 项目已进入收尾 / bug 修复期。本目录不再代表 agent 的默认下一步工作队列。

## 当前规则

默认行为：

1. 用户报告 bug、结果不对、回归失败时，先读 [../MAINTENANCE_MODE.md](../MAINTENANCE_MODE.md)。
2. 不主动继续本目录里的 Phase 计划。
3. 不用“顺手完成 TODO”解释与当前 bug 无关的大改。
4. 只有用户明确说“继续某个 TODO / 继续 Phase / 做历史计划”时，才读取对应文件并恢复功能建设模式。

## 冻结的历史计划

| 文件 | 原状态 | 收尾期处理 |
|------|--------|------------|
| [SYSTEM_REGISTRY_NORMALIZATION_REPORT.md](SYSTEM_REGISTRY_NORMALIZATION_REPORT.md) | doing | 冻结；只在相关 bug 需要理解 registry 语义时读取 |
| [SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md](SYSTEM_ANCHOR_ORCHESTRATION_PLAN.md) | ready | 冻结；不要为单一 bug 泛化 anchor 三态 |
| [CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md](CODEIZED_SYSTEM_ORCHESTRATION_PLAN.md) | ready | 冻结；迷迭香现状以 ADR 0001 和代码为准 |

这些文件仍保留为历史上下文，不删除、不自动归档。若用户明确恢复功能建设，再逐项确认哪些内容仍然符合当前代码状态。

## 新 bug 记录模板

收尾期不建议为每个 bug 新建 TODO。若 bug 需要跨会话跟踪，可在本目录新建 `BUG_*.md`，使用以下模板：

```markdown
# BUG: 简短标题

> 状态：repro-needed | reproduced | fixing | blocked | fixed
> 来源：用户消息 / issue / debug bundle

## 复现

- 命令：
- layout：
- operbox：
- assignment：
- 实际：
- 期望：

## 定位

- 层级：CLI / data / trade / manufacture / search / layout / schedule / export
- 相关文件：

## 修复范围

| 文件/目录 | 动作 |
|-----------|------|
| `path` | 修改 / 新增回归 |

## 验收

- [ ] 命令或测试
- [ ] 回归已补
- [ ] 文档已更新（如需要）

## 完成后

固定后移动到 `docs/ARCHIVE/done/`，或在相关主文档记录现状。
```

## 归档规则

- bug 修复完成：移动到 `docs/ARCHIVE/done/`。
- 历史方案不再采用：移动到 `docs/ARCHIVE/superseded/`。
- 纯历史设计：优先留在 `plans/` 或移动到 `docs/ARCHIVE/plans/`。
