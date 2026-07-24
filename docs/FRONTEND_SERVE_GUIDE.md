# Frontend Serve Guide

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/FRONTEND_CLI.md；docs/INFRA_CLI.md
> 摘要：说明前端 serve worker 的当前使用方式

使用一个长驻 `infra-cli serve`，通过 stdin/stdout 传输一行一个 JSON 对象。stdout 只允许协议帧，stderr 只用于日志。

## 当前落地状态

- `plan.compute` v1 的后端与 Next BFF 实现及本地证据已经完成。
- 目标分支集成、发布部署和真实浏览器验收尚未完成；当前发布包或已安装 Worker 是否支持 v1，必须以 `ping` 返回的版本字段和契约 hash 判断。
- 后续工作见 [Worker 内联 JSON 集成与部署验收](TODO/Worker内联JSON集成与部署验收.md)。该任务和部署 inventory 完成前，legacy 路径继续保留。

## 主请求

下面只展示通过结构 schema 的最小形状，不承诺该单房布局可求解；真实调用必须内联提交完整 layout 和 operbox。

```json
{"id":1,"method":"plan.compute","params":{"schema_version":1,"layout":{"rooms":[{"id":"control","kind":"control_center","level":3}]},"operbox":[{"id":"char_009_12fce","name":"12F","elite":2,"level":30,"own":true,"potential":6,"rarity":2}],"labels":{"layout":"schema-example","operbox":"schema-example"},"options":{"rotation":"abc_12_6_6","top":20,"system_preferences":{},"maa_title":"My schedule"}}}
```

`layout` 和 `operbox` 是内联对象，不接受 caller 文件路径。`options` 可省略；rotation 默认为 ABC，top 默认为 20。layout 为 1 至 64 个房间，operbox 为 1 至 1000 项，top 为 1 至 100，两个 label 均为非空且不超过 200 UTF-8 bytes；request/response 单个 NDJSON frame 均不超过 8 MiB。

完整参数契约位于 [`contracts/plan-compute-v1.schema.json`](../contracts/plan-compute-v1.schema.json)。Worker 在反序列化前执行该 schema，所有 wire 对象拒绝未知字段；重复 ID、kind/product 跨字段一致性等继续由 Rust 领域校验。v1 schema 发布后不可原地扩展，参数变化必须使用新版本。

成功响应：

```json
{"id":1,"ok":true,"elapsed_ms":123,"result":{"schema_version":1,"profile":{},"rotation":{"profile":"abc_12_6_6","daily":{},"shifts":[]},"maa":{}}}
```

- `profile`：现有账号画像 schema v4。
- `rotation`：当前 profile、daily 和班次效率摘要。
- `maa`：可直接下载或用于现有 UI 的 MAA 排班 JSON。

错误响应：

```json
{"id":1,"ok":false,"elapsed_ms":3,"error":{"code":"PLAN_FAILED","stage":"plan.compute","message":"..."}}
```

## 生命周期

1. Next 进程按需启动一个 `infra-cli serve`。
2. 同一客户端只允许一个 in-flight 请求；第二个请求立即返回 `WORKER_BUSY`。
3. execution timeout 会终止 Worker 并失败当前请求，不自动重发。
4. Worker busy 时 health 使用本地 running/busy 状态，不发送 ping。
5. `ping` result 必须包含 `protocol_version=1`、`plan_schema_version=1` 和 `plan_contract_sha256`；hash 可与发布包 `contracts/SHA256SUMS` 对账，缺失表示 Worker 需要升级。
6. 运行记录由 Next 在收到响应后 best-effort 保存，保存失败不改变求解结果。

## Legacy

旧 `method: "plan"` 在兼容期继续接收 operbox/layout/profile_out/maa_out/output_dir 等路径，并保持旧响应。新前端不回退该方法；达到部署版本门禁后按 [Worker 旧路径协议清理](TODO/Worker旧路径协议清理.md) 删除 legacy adapter。
