# `infra-cli serve` 运行与接入

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/接口/CLI与Worker.md；docs/FRONTEND_CLI.md
> 摘要：说明常驻 Worker 的启动、JSONL 边界、探活和进程故障定位
> 代码范围：`crates/infra-cli/src/commands/serve.rs`
> 代码锚点：`serve_cmd`、`read_ndjson_frame`、`write_response`、`handle_request`
> 关联链路：[CLI 与 Worker](接口/CLI与Worker.md)、[前端对接](FRONTEND_CLI.md)、[错误与诊断](接口/错误与诊断.md)
> 核验基线：`serve --help`、`ping`、一条最小 `plan.compute`

## 启动

```bash
infra-cli serve
```

Worker 启动时加载实例表、技能表、练卡知识和 v1–v4 schema；随后尝试预热本地 bake（当前实现可安全不启用）。stderr 只写 ready、加载/预热提示和性能日志。

## 协议边界

- stdin：一行一个 UTF-8 JSON request；EOF 正常结束；空白行忽略。
- stdout：一行一个 JSON response，写完立即 flush；不得输出日志。
- 单帧最大 8 MiB；超限返回 `FRAME_TOO_LARGE` 并消费完整坏帧后继续。
- 非 UTF-8 返回 `INVALID_UTF8`；JSON 解析失败返回外层错误。
- 支持方法：`ping`、`plan`、`plan.compute`。

不要把 `serve` 当 HTTP 服务；HTTP、鉴权和进程守护属于上游部署层。

## 部署检查

1. 发送 `ping` 并记录协议版本、schema hash、solver executable hash、commit 和 build time。
2. 发送最小 `plan.compute`，确认 response 是单行 JSON、`ok` 与 result 形状正确。
3. 对真实请求记录 id、schema version、layout/operbox label 和错误原文。
4. 进程退出或 stdout 非 JSON 时，先检查 stderr 和运行时 data 根，再检查调用方是否误把日志写进 stdout。

Worker 具体字段与错误外形见 [FRONTEND_CLI.md](FRONTEND_CLI.md)；完整调用链见 [调用链/plan请求](调用链/plan请求.md)。
