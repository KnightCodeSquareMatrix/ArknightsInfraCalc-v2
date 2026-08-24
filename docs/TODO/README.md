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
| [Worker 契约 hash 治理与前端版本上报](Worker契约hash治理与前端版本上报.md) | `proposal` | 分两个阶段治理契约校验并补齐构建标识：当前版本前端侧缓解（schema hash 降诊断、feedback 携带本次求解的 solver Git 提交号和构建时间），下一版本从 ping 契约移除 schema hash 并增强 schema 错误报告 |
| [Worker 旧路径协议清理](Worker旧路径协议清理.md) | `blocked` | 等待 plan.compute v1 集成部署和 inventory 后删除 legacy plan 路径协议与兼容说明 |
| [体系注册表后续缺口](体系注册表后续缺口.md) | `ready-on-request` | 承接旧组合体系规范化报告中仍有效但未获当前授权的开放项 |
| [动态 Producer 旧设计阻塞分析交接](动态Producer旧设计阻塞分析交接.md) | `ready-on-request` | 记录回退后的动态 producer 旧模型边界、已验证阻塞和下一位 agent 的只读分析入口 |
| [求解器质量与性能优化建议路线图](求解器质量与性能优化建议路线图.md) | `proposal` | 以证据门禁协调求解质量、联合选择和热路径性能优化建议 |
| [精英干员组后端缺口](精英干员组后端缺口.md) | `ready-on-request` | 后端按实际编制统计精英设施数并结算凯尔希·思衡托「泰拉的方舟」；推荐可先展示、不得宣称求解已支持 |
| [训练室输出与 plan.compute 协议](训练室输出与plan.compute协议.md) | `ready-on-request` | 增加可选 `training_room` 响应字段，保持总体 `plan_schema_version: 3` |
| [罗德岛基建管家 — 网站页面结构设计](网站页面结构设计.md) | `proposal` | 网站页面结构和前端线框图提案，尚未获得实施授权 |
| [罗德岛基建管家](罗德岛基建管家.md) | `proposal` | 一站式基建管家的产品愿景提案，尚未获得实施授权 |
| [跨房计算错误归因与联合求解评估](跨房计算错误归因与联合求解评估.md) | `proposal` | 收集跨房错误样本，定位首次违规层，再判断局部修复或联合求解边界 |
| [近期已知缺口修复清单](近期已知缺口修复清单.md) | `ready-on-request` | 按独立授权逐项恢复的近期已知缺口清单 |
<!-- END GENERATED ACTIVE CHANGES -->

新建任务时优先使用清晰中文名，并声明 current owner、目标、非目标和成功标准。完成后先吸收 current facts、拆开放项，再移动到对应归档目录。
