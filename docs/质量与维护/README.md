# 质量与维护入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/质量与维护/QUALITY_AND_AUDIT.md；docs/质量与维护/MAINTENANCE_MODE.md
> 摘要：按正确性、维护复现、性能、体系审计和证据要求定位质量文档

## 阅读地图

| 问题 | 入口 |
|---|---|
| 交付需要什么保证和证据 | [质量、求解保证与验证证据](QUALITY_AND_AUDIT.md) |
| 已知错误如何复现和选择首个修复层 | [Debug 与一致性修复指南](MAINTENANCE_MODE.md) |
| 热路径、Bake、缓存与性能错误如何区分 | [搜索性能与 Bake 边界](PERFORMANCE_ENGINEERING.md) |
| 体系逐项审计怎么做 | [体系审计工作流](SYSTEM_AUDIT_WORKFLOW.md) |
| 求解失败证据如何记录 | [求解错误分析报告](求解错误分析报告.md) |

质量门禁、工具参数和证据包装入口见 [scripts/codex 使用说明](../../scripts/codex/README.md)。开放质量与性能事项从 [性能质量工作流](../TODO/性能质量.md) 进入。

文档角色、TODO、生成导航和仓库事实检查分别从 [文档生命周期](../文档生命周期.md)、[文档协作工作流](../TODO/文档协作.md) 与 [生成文档目录](../生成索引/文档目录.md) 进入。

## 本领域全部文档

<!-- BEGIN GENERATED DOMAIN DOCUMENTS -->
| 文档 | 角色 | 状态 | 摘要 |
|---|---|---|---|
| [Debug 与一致性修复指南](MAINTENANCE_MODE.md) | `canonical` | `current` | 按症状、调用链和证据阶段修复既有结果错误 |
| [搜索性能与 Bake 边界](PERFORMANCE_ENGINEERING.md) | `current-reference` | `current` | 说明搜索热路径、Bake、缓存、continuation 与验证成本边界 |
| [质量、求解保证与验证证据总则](QUALITY_AND_AUDIT.md) | `canonical` | `current` | 裁决质量、求解保证和交付证据要求 |
| [quality-check CLI 更新规则](QUALITY_CHECK_CLI_UPDATE.md) | `current-reference` | `current` | 记录将求解器 Linux CLI 更新到 quality-check 的操作规则与热路径部署 |
| [公孙长乐体系逐项审计与修复工作流](SYSTEM_AUDIT_WORKFLOW.md) | `canonical` | `current` | 裁决 formal system audit 工作流 |
| [求解错误分析报告](求解错误分析报告.md) | `current-reference` | `current` | 记录求解失败的可复现输入、首个失败阶段、证据和后续动作 |
<!-- END GENERATED DOMAIN DOCUMENTS -->
