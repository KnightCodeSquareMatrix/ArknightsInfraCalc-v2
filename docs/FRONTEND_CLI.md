# 前端与 `plan.compute` 对接

> 文档角色：canonical
> 生命周期状态：current
> 领域键：interface.frontend-cli
> 当前真源：self
> 摘要：定义前端调用 Worker `plan.compute` 的请求、响应、版本和输出边界
> 代码范围：`crates/infra-cli/src/commands/serve.rs`、`commands/plan_compute.rs`、`contracts/plan-compute-v*.schema.json`
> 代码锚点：`handle_request`、`handle_plan_compute`、`PlanComputeParams`、`PlanComputeResult`
> 关联链路：[输入输出与兼容](接口/输入输出与兼容.md)、[plan 请求](调用链/plan请求.md)、[错误与诊断](接口/错误与诊断.md)
> 核验基线：Worker `ping`；同一请求的 `plan.compute`；契约 hash 与 schema version

前端应把 Worker 当作 JSONL 子进程或长驻服务：写一行 request，读取一行 response。不要解析 CLI `plan` 的 stdout，也不要从 MAA 文件反推 profile 字段。

## 启动与探活

```bash
infra-cli serve
```

启动后 stderr 会报告 ready；stdout 保持 JSONL。先发送：

```json
{"id":"ping-1","method":"ping","params":{}}
```

响应包含 `protocol_version`、`supported_plan_schema_versions`、每版本 `plan_contract_sha256`、`solver_executable_sha256`、`solver_git_commit` 和 `solver_built_at`。部署方应记录这些值，用于把前端请求和 solver 制品关联起来。

## `plan.compute` 请求

当前推荐 `schema_version: 5`。完整字段限制见 [输入、输出与兼容契约](接口/输入输出与兼容.md) 和仓库内 `contracts/plan-compute-v5.schema.json`。最小结构：

```json
{"id":"req-1","method":"plan.compute","params":{"schema_version":5,"layout":{"rooms":[{"id":"control","kind":"control_center","level":3}]},"operbox":[{"id":"x","name":"12F","elite":0,"level":30,"own":true,"potential":6,"rarity":2}],"options":{"rotation":"abc_12_6_6","top":20,"fiammetta_enable":true,"support_combinations_enable":false,"assert_invariants":false}}}
```

`layout` 和 `operbox` 是内联值，避免 Worker 依赖调用方机器上的路径。`labels` 只用于日志和性能标识，不是文件路径。`options` 默认 rotation `abc_12_6_6`、top `20`、菲亚启用；未知字段拒绝。

主备 24/24 使用 v5 的 `options.rotation: "main_backup_24_24"`，输出两班各 24h、总周期 48h；daily 仍为 24h 平均产出。较早请求版本不接受这个 profile。展示和执行间隔读取 `rotation.shifts[].duration_hours` 与 MAA 的相对间隔说明。

`options.support_combinations_enable` 为布尔值，默认 `false`。只有显式 `true` 才沿用当前主动补挂件流程；关闭时主角色仍可竞争，技能仍读取当班真实编制。v1–v4 不接受这个新字段，未传入同样默认关闭，这是用户明确接受的排班行为变化。前端使用 v5 并同步开关；结果不增加开关回显。深巡与鸿雪豁免，普通同房组合及思衡托自然读取不受影响。

## 响应与消费

成功响应的 `result` 同时包含：

- `profile`：账号画像（当前 schema 4）；
- `rotation`：daily、shifts、assignment、房间效率；
- `maa`：同一 rotation 的 MAA 投影；
- `training_advice`：由 inline layout、operbox 和当前 rotation 生成的建议。

前端应检查外层 `ok`、`solver` 指纹和 `result.schema_version`。`rotation` 是最终状态来源，`maa` 仅是导出格式；两者出现人员差异时先查 exporter。

## 失败处理

`plan.compute` 求解失败通常返回 `ok=false`、外层 `error.code=PLAN_FAILED`，并在 `result.error` 保留 `code/stage/message`，可能还有 partial rotation/profile。`assert_invariants=true` 时断言失败直接作为外层错误。协议/schema 错误则可能没有 result，调用方必须显示原始 message 并保留 request id。

前端不要重试未知输入错误；对超时或进程退出，应重新探活并记录 solver 指纹。详细分类见 [错误与诊断](接口/错误与诊断.md)。

## 与文件型 CLI 的边界

一体化网页可直接消费 `plan.compute`。需要用户本地文件或 MAA 导出时使用 CLI `plan`，显式传 `--profile-out`、`--maa-out`。`layout-gen` 只生成 `BaseBlueprint`，不执行求解；它不是第二个 solver。

### 4.5 `training_advice`：排班附带的练卡建议

`plan.compute.result.training_advice` 是同一轮 inline layout、operbox 和 rotation 的建议投影。它不改变已经生成的 assignment，也不替代独立 `advice` 入口的兼容诊断。前端只展示后端返回的字段和引用，不自行拼接效率或练度结论。
