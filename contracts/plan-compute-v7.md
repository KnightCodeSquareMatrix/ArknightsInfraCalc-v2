# `plan.compute` 参数契约 v7

> 状态：WIP 契约草案（Worker v7 源码与本文件同属本次 WIP 提交）；未完成 fresh release 验收或部署，消费者仍须按实际 ping 协商，本文件不单独证明 v7 已受支持
> 机器契约：[plan-compute-v7.schema.json](plan-compute-v7.schema.json)
> 实现入口：`infra-cli serve` / `method: "plan.compute"`

v7 保持 v6 的 layout、operbox、labels、options 默认值和结果 envelope。`options.rotation` 继续接受 v6 的六个 named profile 字符串，并新增两种拓扑对象：

```json
{"topology":"main_backup","shift_hours":[8,16]}
```

```json
{"topology":"abc","shift_hours":[6,12,6]}
```

`shift_hours` 按输入顺序对应班次；对象结构只区分两班主备和三班 ABC，数组长度属于拓扑结构，不是时长业务校验。普通泛型周期由调用方限制为 24h、36h 或 48h，后端不新增这项业务校验。泛型班次超过 12h 时该班最多生成一次菲亚动作；ABC 并列最长时休班相位放在较短班。24/24 named 与泛型长班均由实际 ETA/心情准入决定，泛型超过 24h 不复用姓名名单。求解结果在 `rotation.shifts[]` 中逐班返回实际时长与最终 assignment，MAA 直接消费同一报告。

v7 响应中的 `rotation.profile` 是 scalar profile，不是请求体的原样回显。named 字符串返回对应 named profile；generic 对象的 `main_backup` `[12,12]`、`[24,24]`、ABC `[12,6,6]`、`[12,12,12]` 会归一为相应兼容 named profile；其他 generic 主备和 ABC 分别返回 `generic_main_backup` 与 `generic_abc`。实际时长只以 `rotation.shifts[].duration_hours` 为准。

结构解码之后，generic 时长由 core 执行数值安全检查，规则见[定时换班](../docs/定时换班.md)。非法数值返回既有 `PLAN_FAILED` envelope，消息含 `rotation_duration_input:`；不新增总周期组合业务校验，不修改本版机器 schema/hash，也不表示 Worker v7 已部署。

v1–v6 的 schema 文件和 wire 解释不变。需要该扩展的调用方必须确认 `ping.result.supported_plan_schema_versions` 包含 `7`，并按 `plan_contract_sha256_by_version["7"]` 校验机器合同；不得将对象发送到 v6。
