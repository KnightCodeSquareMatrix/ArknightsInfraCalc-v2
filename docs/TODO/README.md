# 准备实现的功能

这个目录放**准备实现、正在实现、需要 AI 接手的事项**。Agent 看到用户说“继续下一步”“做 TODO”“看准备实现的功能”时，优先读本目录，而不是翻完整 `docs/` 或 `plans/`。

## 当前队列

| 优先级 | 文件 | 状态 | 说明 |
|--------|------|------|------|
| P0 | [SYSTEM_REGISTRY_NORMALIZATION_REPORT.md](SYSTEM_REGISTRY_NORMALIZATION_REPORT.md) | doing | Phase 0/1 已完成；下一步拆清跨站 / 全局资源与旧 registry 语义 |

## 新 TODO 文件模板

```markdown
# 功能名

> 状态：ready | doing | blocked
> 来源：链接到设计文档或用户需求

## 目标

- 要达成什么
- 明确不做什么

## 改动范围

| 文件/目录 | 动作 |
|-----------|------|
| `path` | 新增/修改/删除 |

## 验收

- [ ] 命令或测试
- [ ] 文档更新

## 完成后归档

完成后移动到 `docs/ARCHIVE/`，并在相关主文档更新状态。
```

## 归档规则

- 已完成：移动到 `docs/ARCHIVE/done/`。
- 被放弃或方案不采用：移动到 `docs/ARCHIVE/superseded/`。
- 纯历史设计：优先留在 `plans/` 或移动到 `docs/ARCHIVE/plans/`。
