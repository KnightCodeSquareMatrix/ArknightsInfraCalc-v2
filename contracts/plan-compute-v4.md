# `plan.compute` 参数契约 v4

> 状态：已发布、不可变  
> 机器契约：[plan-compute-v4.schema.json](plan-compute-v4.schema.json)  
> 实现入口：`infra-cli serve` / `method: "plan.compute"`

v4 完整继承 v3 的 layout、operbox、labels、rotation、top、system_preferences、maa_title 和 fiammetta_enable 语义，只新增一个可选调试开关：

```json
{
  "schema_version": 4,
  "layout": { "rooms": [] },
  "operbox": [],
  "options": {
    "assert_invariants": true
  }
}
```

`options.assert_invariants` 为 boolean，默认 `false`。为 `true` 时，Worker 在完成求解后、生成 profile/MAA/training_advice 输出前，对最终 `TeamRotationReport.shifts[].assignment` 运行与 CLI `plan --assert-invariants` 相同的 Rust invariant gate。失败返回正常 `plan.compute` 错误 envelope：`code=PLAN_FAILED`、`stage=plan.compute`，`message` 保留具体 `assert-invariants ...` 诊断。

调用方只有在 `ping.result.supported_plan_schema_versions` 包含 `4` 时才能发送该字段。v1-v3 请求及默认行为保持不变。
