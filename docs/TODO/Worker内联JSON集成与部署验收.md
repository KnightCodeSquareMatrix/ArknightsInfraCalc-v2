# Worker 内联 JSON 集成与部署验收

> 文档角色：active-change
> 生命周期状态：ready-on-request
> 当前真源：docs/FRONTEND_CLI.md；docs/FRONTEND_SERVE_GUIDE.md
> 摘要：将已验证的 plan.compute v1 集成到目标分支和部署环境并完成浏览器验收

## 当前状态

- 后端共享 Plan 编排、`plan.compute` v1、参数 JSON Schema admission 和 legacy adapter 已在协作工作区实现并通过本地证据。
- Next BFF 已在独立集成工作区改为内联请求和响应；现有浏览器 API/UI 契约保持不变，本地 Full E2 与负向边界已经通过。
- 目标分支集成、正式发布包、目标环境部署和真实浏览器验收尚未完成，不能据此宣称全链路迁移已经上线。
- 已安装 Worker 的能力必须通过 `ping.protocol_version=1`、`ping.plan_schema_version=1` 与目标发布 schema 的 `ping.plan_contract_sha256` 共同判断。

## 目标

把已完成的后端与前端实现集成到各自目标分支，发布匹配版本，在目标环境验证 Browser -> Next BFF -> `infra-cli serve` -> `plan.compute` 主链路，并形成可供 legacy 退役判断的部署 inventory。

## 执行项

1. 审阅后端与前端最终 diff，将两侧实现集成到预定目标分支，并记录可追溯 revision。
2. 从已集成 revision 构建并发布匹配的 `infra-cli` 与 Next BFF，确认运行配置指向该 Worker。
3. 在目标环境验证 health/ping 版本字段与契约 hash、真实 `/api/plan`、profile v4、非空且等长的 rotation shifts / MAA plans，以及浏览器排班展示和 MAA 下载。
4. 按下节 inventory 要求记录每个目标环境的版本、协议遥测、观测窗口和回滚边界。
5. 更新 release 版本信息和本任务引用的 current 状态；验收完成后按文档生命周期归档本任务。

## 部署 inventory

本 active change 是部署 inventory 的 owner；执行时在本节追加环境记录，原始日志和命令通过 evidence 工具留痕。任务关闭后，inventory 摘要随本文移入 archive，current owner 只保留最终发布状态。

每个目标环境必须记录：

- 环境标识、访问入口和部署时间；
- 前端 revision、Worker revision、发布版本或产物 hash；
- `ping.protocol_version`、`ping.plan_schema_version`、`ping.plan_contract_sha256` 的实际值和采样时间；
- 可区分 `plan.compute` 与 legacy `plan` 的日志或指标来源、查询条件、观测窗口起止时间；
- 两种 method 的调用量、可识别的调用方版本，以及无法归属的调用方；
- 失败时可恢复的前端与 Worker revision / 产物。

观测必须覆盖全部目标环境和已知前端发布渠道，并至少覆盖该部署实际采用的最长客户端缓存、灰度或回滚窗口。任一环境没有 method 遥测、观测窗口未知、仍有 legacy 调用或存在无法归属的调用方时，只能完成 v1 上线验收，不能解除 legacy 清理任务的 `blocked` 状态。

## 非目标

- 本任务不删除 legacy `method: "plan"`；退役由 [Worker 旧路径协议清理](Worker旧路径协议清理.md) 独立执行。
- 不恢复 production flow/source stock，不重做 ScheduleView，不新增远程 transport、队列、progress 或 cancel。
- 不把本地测试通过等同于目标环境部署成功。

## 完成条件

- 后端与前端目标分支均包含可追溯的 v1 实现 revision，发布产物来自这些 revision。
- 目标环境 `ping` 明确返回两个 v1 版本字段及与发布 schema 一致的契约 hash，真实浏览器 Plan 流程和 MAA 下载通过。
- 全部目标环境的 deployment inventory 已按本节字段记录；legacy 清理任务能据此保持 `blocked`，或在零 legacy 调用且无未知调用方时转为 `ready-on-request`。
- 发布状态已吸收到 current owner，证据、索引和引用通过生命周期检查。
