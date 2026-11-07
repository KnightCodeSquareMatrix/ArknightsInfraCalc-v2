# 接入与前端入口

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/接入与前端/INFRA_CLI.md；docs/接入与前端/FRONTEND_CLI.md；docs/接口/CLI与Worker.md
> 摘要：定位 CLI、Worker、前端接入、字段迁移和部署验收的当前文档

## 阅读地图

| 场景 | 入口 |
|---|---|
| CLI 命令与 core 调用边界 | [infra-cli 模块职责与命令地图](INFRA_CLI.md) |
| 前端调用 `plan.compute` 的协议与版本 | [前端 plan.compute 对接](FRONTEND_CLI.md) |
| 启动常驻 Worker、JSONL、探活 | [前端 serve 指南](FRONTEND_SERVE_GUIDE.md) |
| Next 前端真实接入操作 | [真实求解器接入说明](前端真实求解器接入说明.md) |
| 统一效率字段迁移 | [前端效率字段迁移说明](前端效率字段迁移说明.md) |
| wire schema、错误和诊断 | [接口目录](../接口/CLI与Worker.md) |

开放接口与产品工作从 [接口产品工作流](../TODO/接口产品.md) 进入；提案不替代当前 contract。

## 本领域全部文档

<!-- BEGIN GENERATED DOMAIN DOCUMENTS -->
| 文档 | 角色 | 状态 | 摘要 |
|---|---|---|---|
| [前端与 `plan.compute` 对接](FRONTEND_CLI.md) | `canonical` | `current` | 定义前端调用 Worker `plan.compute` 的请求、响应、版本和输出边界 |
| [`infra-cli serve` 运行与接入](FRONTEND_SERVE_GUIDE.md) | `current-reference` | `current` | 说明常驻 Worker 的启动、JSONL 边界、探活和进程故障定位 |
| [infra-cli 模块职责与命令地图](INFRA_CLI.md) | `canonical` | `current` | 说明 infra-cli 的分层职责、命令入口和 core 调用边界 |
| [前端效率字段迁移说明](前端效率字段迁移说明.md) | `current-reference` | `current` | 说明前端迁移到统一效率字段的读取、展示和验收要求 |
| [前端真实求解器接入说明](前端真实求解器接入说明.md) | `current-reference` | `current` | 说明 Next 前端接入本地或服务器 `infra-cli serve` 的操作清单 |
<!-- END GENERATED DOMAIN DOCUMENTS -->
