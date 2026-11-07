# Worker 内联 JSON 协议迁移

> 文档角色：archive
> 生命周期状态：completed
> 当前真源：docs/接入与前端/FRONTEND_CLI.md
> 替代项：docs/接入与前端/FRONTEND_CLI.md；docs/接入与前端/INFRA_CLI.md
> 历史原因：实现与本地验证已完成；集成部署和 legacy 退役分别由独立 active change 跟踪
> 快照日期：2026-07-22
> 摘要：统一 CLI 与常驻 Worker 的 Plan 编排，并将前端主链路迁移到无路径的内联 JSON 请求和响应

## 进度

- [x] clean HEAD 与前端调用面已通过 Terra 只读核对。
- [x] 用户裁决 v1 暂缓 production flow/source stock。
- [x] 抽取共享 Plan 编排，消除 CLI 的重复 rotation。
- [x] legacy Worker 改用共享编排并保持旧契约。
- [x] 新增内联 `plan.compute` 和契约测试。
- [x] Next BFF 切换到 `plan.compute`，浏览器 API/UI 不变。
- [x] 完成最终证据并拆分集成部署与 legacy 清理项。
- [x] 归档本文并通过生命周期 hard check。

## 实施前基线

- Browser -> Next.js Node route -> 同机 `infra-cli serve` 子进程。
- 浏览器到 Next 已是 JSON；Next 当前将 layout/operbox 落盘，向 Worker 传路径，再读取 profile/MAA/shifts 文件。
- clean HEAD 的 CLI `plan` 为 profile 和排班分别运行一次 rotation；legacy Worker 已使用单次 probe 后派生 profile/MAA。
- clean HEAD 没有 production flow/source stock；固定 2/3/4 班 profile 已存在。
- legacy Worker 已内联 shifts 与 daily，但 profile/MAA 仍只写文件。
- 前端仍以 MAA 作为房间排班展示模型，本任务不重做 ScheduleView。

## 已确认实现

- `commands/plan_compute.rs` 是 CLI/Worker 唯一 Plan 编排，profile 与 MAA 消费同一次 user rotation。
- `plan.compute` v1 内联接收 layout/operbox，返回 profile v4、完整兼容 RotationShift 摘要和 MAA；legacy `plan` 仍按路径兼容。
- Next BFF 不再为主链路创建或读取 layout/operbox/profile/MAA/shifts 文件，运行记录改为响应后 best-effort 保存。
- Next 同时只允许一个 in-flight Plan；busy health 不 ping，timeout/close 不自动重发。
- production flow/source stock 不在 clean HEAD v1 中；实施时已隔离并明确延期。

## 不变量

1. 一次 Plan 只生成一次 rotation；profile、MAA 和 rotation 摘要消费同一结果。
2. 共享编排留在 `infra-cli` 私有模块，只组合现有 core API，不实现机制或新 policy。
3. Worker wire DTO 不进入 `infra-core`。
4. `plan.compute` 内联接收 layout/operbox，内联返回 profile、rotation 摘要和 MAA，不接受 caller 文件路径。
5. CLI 文件 I/O、人类输出和 legacy 文件协议继续是 adapter；兼容期内旧契约不变。
6. Next BFF 映射回现有 `PlanApiResponse`，`src/api.ts`、React UI、schedule/skland 消费者首次迁移不改。
7. stderr 只作日志；结构化成功结果来自响应 JSON。

## v1 契约

请求沿用现有 NDJSON envelope：

```json
{"id":1,"method":"plan.compute","params":{"schema_version":1,"layout":{},"operbox":[],"labels":{"layout":"243","operbox":"Full E2"},"options":{"rotation":"abc_12_6_6","top":20,"system_preferences":{},"maa_title":"Arknights InfraCalc"}}}
```

成功响应：

```json
{"id":1,"ok":true,"elapsed_ms":123,"result":{"schema_version":1,"profile":{},"rotation":{"profile":"abc_12_6_6","daily":{},"shifts":[]},"maa":{}}}
```

错误保留现有 envelope，增加简单 `code`、`stage`、`message`。`ping` result 增加 `protocol_version: 1` 和 `plan_schema_version: 1`。

`rotation` 只投影 profile、daily、每班 index/duration、active/resting teams、weighted 指标和 `efficiencies.room_lines`；不暴露 `peak_plan`、`assignment_plans` 或完整 assignment。profile 保持 schema v4，MAA 保持外部 MAA JSON。

基础门禁：Next 请求体不超过 2 MiB；request/response NDJSON frame 均不超过 8 MiB；layout 为 1 至 64 个房间；operbox 为 1 至 1000 项；`top` 为 1 至 100；两个 label 均为非空且不超过 200 UTF-8 bytes。

## 实施步骤

### A. 共享编排

- 新增 `crates/infra-cli/src/commands/plan_compute.rs`，定义 `PlanComputeInput`、`RequestedOutputs`、`ComputedPlan` 和 `compute_plan`。
- 以 legacy Worker 的单次 `run_user_rotation_probe_*` 路径为基准，再从同一 probe 派生 profile 和可选 MAA。
- CLI `plan` 始终请求 profile，按 `--maa-out` 请求 MAA；legacy Worker 按 `profile_out`/`maa_out` 请求对应产物。
- `plan.rs` 只保留 argv/path、文件写出和 renderer；`serve.rs` 只保留 wire/path adapter。

### B. `plan.compute`

- 在 `serve.rs` 增加 v1 DTO 和 dispatcher，内联反序列化 `BaseBlueprint` 与规范化 operbox。
- bundle 内联 profile、MAA 和精简 rotation；成功不依赖输出文件。
- stdin 使用有界 frame；stdout 只写协议帧；Worker 不增加 retry。
- legacy `plan`/`ping` 原样兼容，新 ping 只增加字段。

### C. Next BFF

- 前端优先只改 `src/server/infra.ts`，必要时小改 health/debug 类型。
- `runPlan` 发送内联 `plan.compute`，不再创建主链路输入/产物路径。
- response 映射为现有 `PlanApiResponse`；运行记录改为响应后 best-effort 保存。
- 同时只允许一个 pending；第二请求返回 `WORKER_BUSY`；超时 kill 且不自动 resend；busy health 不 ping。

### D. 验证与关闭

- 后端：格式、共享编排测试、legacy/new contract、`cargo test -p infra-cli`、相关 core tests、workspace check、真实 serve smoke。
- 前端：test、lint、build、真实 Full E2 `/api/plan`，并验证 busy/health/timeout/持久化失败边界。
- 所有结论通过 `arknights-evidence` 留痕。
- 实现事实吸收到后端 `FRONTEND_CLI.md`、`INFRA_CLI.md`、`PROJECT_MAP.md` 和前端 Serve Guide/README/AGENTS。
- 完成后将本文归档；集成部署和 legacy 删除分别拆成独立 active change，后者须等新前端部署达到最低版本。

## 验证结果

- 后端 `cargo fmt --all -- --check`、`cargo test -p infra-cli`、`cargo check --workspace`、legacy/new serve smoke 和 diff check 通过。
- 当前 `cargo test -p infra-core` 为 554 passed / 7 failed；本轮未改 `infra-core`，且七个精确失败测试均已在 detached clean `HEAD` `181ed63` 复现，因此作为既有基线保留，不伪装为通过。
- 前端 test、lint、build、diff check 和真实 Worker Full E2 通过；Full E2 返回 profile v4、3 个 rotation shifts、3 个 MAA plans 和非空 room lines。
- 负向证据覆盖第二请求 `WORKER_BUSY`、busy health 不 ping、timeout/close 各只发送一次且不重试、持久化失败不改变 success、流式 2 MiB 精确边界、旧 Worker 拒绝、损坏或空 profile/MAA/rotation 拒绝、shift/plan 数量不一致拒绝及 UTF-8 label 截断；测试 Worker 与 body checker 均保存在前端仓库 `fixtures/`。
- 集成与部署验收已拆至 `docs/TODO/详情/接口产品/Worker内联JSON集成与部署验收.md`，状态为 `ready-on-request`。
- legacy 删除已拆至 `docs/TODO/详情/接口产品/Worker旧路径协议清理.md`，状态为 `blocked`，部署 inventory 仍是删除门禁。
- 后端证据 manifest：`target/codex-runs/worker-inline-json-implementation/manifest.json`；前端证据 manifest：`target/codex-runs/worker-inline-json-frontend/manifest.json`。

## 非目标与延期

- production flow/source stock；独立 ScheduleView；progress/cancel/队列/远程 transport。
- 前后端 xlsx 统一；JSON Schema/TS 自动生成；完整 resource revision；公开部署认证。
- `support_facilities` 及其他无关 TODO。

## 完成条件

- CLI 与 Worker 共享唯一 Plan 编排，CLI 不再重复 rotation。
- `plan.compute` 在无 caller 文件路径时返回完整前端所需 JSON。
- Next 主链路不再依赖 Worker 输出文件，现有 UI 行为保持。
- legacy 兼容有测试和明确清理门禁；延期项未被伪装为已实现。
