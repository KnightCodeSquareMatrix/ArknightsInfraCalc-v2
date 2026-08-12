# Worker 契约 hash 治理与前端版本上报

> 文档角色：active-change
> 生命周期状态：proposal
> 当前真源：docs/FRONTEND_CLI.md
> 摘要：分两个阶段治理契约校验并补齐构建标识：当前版本前端侧缓解（schema hash 降诊断、feedback 携带实际运行的 solver 可执行制品指纹），下一版本从 ping 契约移除 schema hash 并增强 schema 错误报告

## 背景

前端 `src/server/plan-protocol.ts` 硬编码 `PLAN_CONTRACT_SHA256`，运行时与 solver `ping` 返回的 `plan_contract_sha256` 比对，不符则判 `plan.compute` 不支持并静默降级到 legacy `plan` 路径。

实测该 hash 是 solver 内嵌 JSON Schema **原始字节**的 SHA-256。schema 语义未变，仅 LF/CRLF 与空白差异就会改变 hash（本地 LF=`60acbcf1`，已部署 CRLF=`52b78160`），导致前端误判降级、无报警。

## 用户裁决

- 当前版本保持已发布 `plan.compute` v1 行为不回改。
- 当前版本由前端侧缓解：`plan_contract_sha256` 不再作为运行时能力判定依据，降为诊断/对账字段；feedback 携带 solver 构建标识，便于按版本定位问题。
- 下一版本移除前后端对 `plan_contract_sha256` 的契约绑定与验证，保留构建标识字段。
- 任何阶段都不移除 JSON Schema 参数 admission；`protocol_version` 和 `plan_schema_version` 继续承担兼容性判断。
- 前端自己的实现细节由前端团队决定；本仓库只定义契约口径、目标与字段。

## 构建标识语义

- `solver_executable_sha256` 是 solver 进程启动时对**当前可执行文件字节**计算的 SHA-256。它精确标识实际运行的 solver 制品，可区分同 schema 的不同二进制。
- `plan_contract_sha256` 只定位**内嵌 schema 字节**，不区分同 schema 的不同 solver 构建。
- 运行时可从外部目录加载数据（见 `docs/MAINTENANCE_MODE.md`），因此可执行制品指纹不覆盖外部数据身份。
- 不使用 git revision、构建时间、mtime 或文件尺寸冒充构建身份；不新增 buildinfo 命令。

## 阶段一：当前版本前端侧修改建议（给前端团队）

### 1. 移除运行时契约 hash 门禁

`inspectPlanComputeCapability` 只校验 `protocol_version === 1` 与 `plan_schema_version === 1`；`contractSha256` 不再作为 `supported` 的判定条件。

版本不兼容时**显式报错并报警**，不要静默回退 legacy `plan`。具体报错形态与前端降级分支由前端团队自行决定。

### 2. schema hash 降为诊断字段

- 保留读取 `plan_contract_sha256`，记录到健康检查与运行日志，但**不阻断请求**。
- 发布/部署对账继续使用发布包 `contracts/SHA256SUMS`（制品完整性），与运行时能力无关。
- 部署检查：将发布清单记录的预期 `solver_executable_sha256` 与 ping 自报值比对，mismatch 显式失败部署或报警，但不改变 `plan.compute` 路由。

### 3. feedback 携带 solver 构建标识

`POST /api/feedback` 写入的 `meta.json` 追加 solver 构建标识：

```json
{
  "solver": {
    "protocol_version": 1,
    "plan_schema_version": 1,
    "plan_contract_sha256": "52b78160...",
    "solver_executable_sha256": "<current executable SHA-256>",
    "observed_at": "2026-08-12T02:00:00Z"
  }
}
```

来源：每次 `runPlan` 时缓存最近一次 `ping` 的 result，或 health 记录；无记录时置 null，不阻塞反馈。过渡期允许缺失，但目标部署验收不以"尽力记录"为最终保证，应以 `solver_executable_sha256` 为准。

## 阶段二：下一版本 solver 侧改动

1. 从 Worker `ping` 契约和对应前端能力检查中移除 `plan_contract_sha256`，**保留** `solver_executable_sha256`。
2. 删除仅为运行时契约 hash 服务的后端计算与依赖；发布包 checksum 若保留，只作为制品完整性信息，不参与 API 兼容判断。
3. 保留 `plan.compute.params` 的 JSON Schema admission。
4. Schema 不匹配时返回包含字段路径和失败原因的错误报告；合法参数不产生额外报告。
5. 更新前端接入文档、Markdown 契约、部署验收项和相关测试。

## 实施前决策

- 错误报告沿用 `PLAN_FAILED`，还是引入单独的参数错误 code。
- 错误报告只返回首个问题，还是返回有上限的问题列表。
- 移除 `ping` 的 `plan_contract_sha256` 是否需要提升 `protocol_version`，以及旧前端的过渡窗口。
- 阶段一前端改动与阶段二的时间边界：阶段一落地并部署、旧调用方退出后，才进入阶段二移除。

这些决策只影响错误和迁移契约，不得借机改变 solver、layout、operbox 或排班语义。

## 非目标

- 不取消 JSON Schema 参数规范。
- 不把 response、profile、rotation 或 MAA 一并 schema 化。
- 不恢复前端拼接 CLI 业务参数。
- 不顺带删除 legacy `method: "plan"`。
- 不新增 `buildinfo` 命令。
- 不做"规范化 hash"（JSON canonicalization 成本高于收益）。
- 不用 git revision、构建时间、mtime 或文件尺寸冒充精确构建身份。
- 不改动 solver 求解、layout、operbox 或排班语义。

## 完成条件

- 阶段一：
  - 更换 schema 字节格式（LF/CRLF）的 solver 后，前端仍走 `plan.compute`，不降级、不报警；schema mismatch 在部署检查或健康诊断中明确报告，不影响用户请求。
  - 两个同 schema 的不同 solver 二进制得到不同的 `solver_executable_sha256`。
  - feedback 的 `meta.json` 含 `solver.solver_executable_sha256`，与 ping 返回一致。
  - 版本号不匹配时前端显式报错，日志含原因。
- 阶段二：当前版本保持原样，改动只进入明确的新版本；新版本合法 `plan.compute` 请求无需 schema hash 对账即可执行；不合规请求返回稳定、可测试且包含字段定位的错误报告；`protocol_version` / `plan_schema_version` 的兼容规则及迁移顺序已明确；后端真实 `serve` 入口、前端 BFF 入口和发布部署流程均有对应回归证据。
