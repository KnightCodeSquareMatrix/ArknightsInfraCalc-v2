# 前端 contractHash 对账与版本上报修改建议

> 文档角色：archive
> 生命周期状态：superseded
> 当前真源：docs/FRONTEND_CLI.md
> 摘要：给前端团队的接口修改建议：feedback 携带 solver 构建版本、移除运行时契约 hash 门禁、更新防呆改为发布对账
> 替代项：[Worker 契约 hash 治理与前端版本上报](../../TODO/Worker契约hash治理与前端版本上报.md)

## 背景

当前前端在 `src/server/plan-protocol.ts` 硬编码 `PLAN_CONTRACT_SHA256`，运行时与 solver `ping` 返回的 `plan_contract_sha256` 比对，不符则判 `plan.compute` 不支持并静默降级到 legacy `plan` 路径。

实测该 hash 是 solver 内嵌 JSON Schema **原始字节**的 SHA-256。schema 语义未变，仅 LF/CRLF 与空白差异就会改变 hash（本地 LF=`60acbcf1`，已部署 CRLF=`52b78160`），导致前端误判降级、无报警。

## 目标

1. **feedback 精确到构建版本**：前端上报 feedback 时携带 solver 构建标识，便于按版本定位问题。
2. **更新防呆**：部署换错二进制或 schema 漂移时能被发现，但不以前端运行时降级为代价。

## 建议的接口行为（目标口径）

| 字段 | 角色 | 用途 |
|---|---|---|
| `protocol_version` | 兼容性判定（必需） | 协议不兼容时显式失败 |
| `plan_schema_version` | 兼容性判定（必需） | schema 语义版本判定 |
| `plan_contract_sha256` | **诊断字段（可选）** | 构建标识 / 发布对账，不参与调用路由 |

## 修改建议

### 1. 移除运行时契约 hash 门禁

`inspectPlanComputeCapability` 只校验 `protocol_version === 1` 与 `plan_schema_version === 1`；`contractSha256` 不再作为 `supported` 的判定条件。

版本不兼容时**显式报错并报警**，不要静默回退 legacy `plan`。

### 2. hash 降为诊断字段

- 保留读取 `plan_contract_sha256`，记录到健康检查与运行日志，但**不阻断请求**。
- 发布/部署对账继续使用发布包 `contracts/SHA256SUMS`（制品完整性），与运行时能力无关。

### 3. feedback 携带构建版本

`POST /api/feedback` 写入的 `meta.json` 追加 solver 构建标识：

```json
{
  "solver": {
    "protocol_version": 1,
    "plan_schema_version": 1,
    "plan_contract_sha256": "52b78160..."
  }
}
```

来源：每次 `runPlan` 时缓存最近一次 `ping` 的 result，或 health 记录；无记录时置 null，不阻塞反馈。

## 明确不做

- 不新增 `buildinfo` 命令（`serve ping` 已返回全部字段）。
- 不做"规范化 hash"（JSON canonicalization 成本高于收益）。
- 不删除 `ping` 的 hash 字段（保留供对账与诊断）。
- 不改动 solver 求解、layout、operbox 或排班语义。

## 验收

- 更换一个 schema 字节格式（LF/CRLF）的 solver 后，前端仍走 `plan.compute`，不降级、不报警。
- feedback 的 `meta.json` 含 `solver.plan_contract_sha256`，与 `ping` 返回一致。
- 版本号不匹配时前端显式报错，日志含原因。
