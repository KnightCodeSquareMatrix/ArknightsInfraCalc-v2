# TODO 任务目录

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md
> 摘要：说明 TODO 目录并承载生成的活动变更表

本目录只容纳 `active-change`。它不是默认工作队列；只有用户当前指令明确选择某项任务时，Agent 才获得实施授权。状态、恢复、交接、关闭和自动归档规则统一见 [文档生命周期](../文档生命周期.md)。

`in-progress` 只用于当前有 writer 的工作树，不能进入默认分支。未完成的会话在交接前改为 `ready-on-request` 或有明确原因的 `blocked`。

## 当前活动变更

<!-- BEGIN GENERATED ACTIVE CHANGES -->
| 文件 | 状态 | 用途 |
|---|---|---|
| [设施无关条件化响应 Bake 实施计划](CONDITIONAL_ROOM_RESPONSE_BAKE_PLAN.md) | `ready-on-request` | 设施无关条件化响应 Bake 的后续实施任务 |
| [动态 Producer A+：设施无关候选列 + 精确索引 Join](DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md) | `ready-on-request` | 动态 producer 候选列、精确索引 Join 和 winner dependency 任务 |
| [Worker 内联 JSON 集成与部署验收](Worker内联JSON集成与部署验收.md) | `ready-on-request` | 将已验证的 plan.compute v1 集成到目标分支和部署环境并完成浏览器验收 |
| [Worker 旧路径协议清理](Worker旧路径协议清理.md) | `blocked` | 等待 plan.compute v1 集成部署和 inventory 后删除 legacy plan 路径协议与兼容说明 |
| [下一版本简化 Worker 契约校验](下一版本简化Worker契约校验.md) | `ready-on-request` | 下一版本移除运行时契约 hash 验证，仅对不符合 JSON Schema 的参数输出错误报告 |
| [体系注册表后续缺口](体系注册表后续缺口.md) | `ready-on-request` | 承接旧组合体系规范化报告中仍有效但未获当前授权的开放项 |
| [练卡推荐规则来源精修](练卡推荐规则来源精修.md) | `ready-on-request` | 逐步补齐已确认练卡规则的具体来源、收益标签和技能元数据 |
| [罗德岛基建管家 — 网站页面结构设计](网站页面结构设计.md) | `proposal` | 网站页面结构和前端线框图提案，尚未获得实施授权 |
| [罗德岛基建管家](罗德岛基建管家.md) | `proposal` | 一站式基建管家的产品愿景提案，尚未获得实施授权 |
| [跨房计算错误归因与联合求解评估](跨房计算错误归因与联合求解评估.md) | `proposal` | 收集跨房错误样本，定位首次违规层，再判断局部修复或联合求解边界 |
| [近期已知缺口修复清单](近期已知缺口修复清单.md) | `ready-on-request` | 按独立授权逐项恢复的近期已知缺口清单 |
<!-- END GENERATED ACTIVE CHANGES -->

新建任务时优先使用清晰中文名，并声明 current owner、目标、非目标和成功标准。完成后先吸收 current facts、拆开放项，再移动到对应归档目录。
