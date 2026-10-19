# `plan.compute` 参数契约 v6

> 状态：当前契约
> 机器契约：[plan-compute-v6.schema.json](plan-compute-v6.schema.json)
> 实现入口：`infra-cli serve` / `method: "plan.compute"`

v6 继承 v5 的 layout、operbox、labels、`support_combinations_enable` 和 `assert_invariants` 字段。当前 `options.rotation` 可用 profile 与 `TimedRotationProfile` 的六项产品闭集一致；独立 `fiammetta_witch_12_12` 已删除。

历史 schema v3–v5 文件不可变，仍会结构性接受旧 profile wire 并将它解码为只供拒绝使用的 tombstone；这不是排班支持。当前 Worker 对 `fiammetta_witch_12_12` 明确返回 `PLAN_FAILED` / `plan.compute`，不会静默映射到 `main_backup_12_12` 或以普通双 owner fallback 继续求解。文件型 `plan`、`layout team-rotation`、debug rerun 与 `infra-core` 的公共排班入口也明确拒绝旧标记。普通菲亚 ABC 分支及贸易站的普通同房组合不受影响。

兼容协商应以 `ping.result.supported_plan_schema_versions` 和对应契约 hash 为准；当前 Worker 声明支持 v1–v6。v6 使用 `support_combinations_enable` 的原有默认关闭语义，不改变其他 profile 的请求与输出。
