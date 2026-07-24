# Worker 旧路径协议清理

> 文档角色：active-change
> 生命周期状态：blocked
> 当前真源：docs/FRONTEND_CLI.md
> 摘要：等待 plan.compute v1 集成部署和 inventory 后删除 legacy plan 路径协议与兼容说明

## 当前边界

- 新 Next BFF 的实现已只调用内联 `plan.compute`，不会回退旧路径协议；本地证据已完成，但目标分支集成和部署验收仍由 [Worker 内联 JSON 集成与部署验收](Worker内联JSON集成与部署验收.md) 跟踪。
- 后端仍保留 `method: "plan"`，用于已发布旧前端的过渡兼容。
- 当前没有部署 inventory 可以证明旧调用方已经退出，因此本任务不可执行。

## 阻塞项

- [Worker 内联 JSON 集成与部署验收](Worker内联JSON集成与部署验收.md) 尚未完成。
- 删除前必须消费该任务保存的 deployment inventory：全部目标环境均支持 `protocol_version=1`、`plan_schema_version=1` 和目标发布契约 hash，method 遥测覆盖已声明观测窗口，legacy `plan` 调用量为零且没有未知调用方。
- 任一环境缺少 method 遥测、观测窗口不足或仍有旧调用方时，本任务继续保持 `blocked`。

## 责任边界

本任务只消费已经完成的发布 revision、浏览器 UAT 和 deployment inventory，不重复构建、部署或验收 v1；只负责确认退役门禁、删除兼容代码并验证删除后的主入口。

## 完成条件

- 集成任务归档的 deployment inventory 覆盖全部目标环境和已声明观测窗口，并证明 legacy `plan` 调用量为零且没有未知调用方。
- 删除后端 `PlanParams`、旧 path adapter、旧 `PlanResult` 和对应 contract test。
- 删除两仓指南中的 legacy 段落，并通过真实 `plan.compute` 前端入口验收。
