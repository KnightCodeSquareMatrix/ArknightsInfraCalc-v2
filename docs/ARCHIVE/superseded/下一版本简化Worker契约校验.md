# 下一版本简化 Worker 契约校验

> 文档角色：archive
> 生命周期状态：superseded
> 当前真源：docs/接入与前端/FRONTEND_CLI.md
> 摘要：下一版本移除运行时契约 hash 验证，仅对不符合 JSON Schema 的参数输出错误报告
> 替代项：[Worker 契约 hash 治理与前端版本上报](../../TODO/详情/接口产品/Worker契约hash治理与前端版本上报.md)

## 用户裁决

- 当前版本保持不变，不回改已发布的 `plan.compute` v1 行为。
- 下一版本移除前后端对 `plan_contract_sha256` 的契约绑定与验证。
- JSON 参数符合规范时正常进入现有 Serde、领域校验和求解流程。
- JSON 参数不符合规范时输出可定位问题的错误报告。

这里移除的是契约原始 bytes 的 hash 验证，不是移除判断参数是否符合 JSON Schema 的能力。`protocol_version` 和 `plan_schema_version` 继续承担兼容性判断。

## 目标改动

1. 从 Worker `ping` 契约和对应前端能力检查中移除 `plan_contract_sha256`。
2. 删除仅为运行时契约 hash 服务的后端计算与依赖；发布包 checksum 若保留，只作为制品完整性信息，不参与 API 兼容判断。
3. 保留 `plan.compute.params` 的 JSON Schema admission。
4. Schema 不匹配时返回包含字段路径和失败原因的错误报告；合法参数不产生额外报告。
5. 更新前端接入文档、Markdown 契约、部署验收项和相关测试。

## 实施前决策

- 错误报告沿用 `PLAN_FAILED`，还是引入单独的参数错误 code。
- 错误报告只返回首个问题，还是返回有上限的问题列表。
- 移除 `ping` 字段是否需要提升 `protocol_version`，以及旧前端的过渡窗口。

这些决策只影响错误和迁移契约，不得借机改变 solver、layout、operbox 或排班语义。

## 非目标

- 不在当前版本实施。
- 不取消 JSON Schema 参数规范。
- 不把 response、profile、rotation 或 MAA 一并 schema 化。
- 不恢复前端拼接 CLI 业务参数。
- 不顺带删除 legacy `method: "plan"`。

## 完成条件

- 当前版本保持原样，改动只进入明确的新版本。
- 新版本合法 `plan.compute` 请求无需 hash 对账即可执行。
- 不合规请求返回稳定、可测试且包含字段定位的错误报告。
- `protocol_version` / `plan_schema_version` 的兼容规则及迁移顺序已明确。
- 后端真实 `serve` 入口、前端 BFF 入口和发布部署流程均有对应回归证据。
