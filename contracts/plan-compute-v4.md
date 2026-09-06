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

非 assert 请求的求解或结果阶段失败仍返回顶层 `ok=false`；当 core 已真实构造 `TeamRotationReport` 时，响应会额外在 `result.error` 重复错误并保留 `result.rotation`，只输出已经成功生成的其它字段。若失败发生在报告产生前，`result.rotation` 为 `null`；不会伪造 assignment，也不会复用旧结果。成功响应和严格 assert 失败响应的既有字段语义不变。

成功响应的 `rotation.shifts[].assignment.rooms[].notes[]` 由 `infra-core::notes` 在最终班次状态上统一生成。每条 note 包含稳定 `code` 和展示用 `text`；`notes` 为空时省略。当前不会把 notes 写入外部 MAA wire JSON，MAA 仍只消费最终人员名单和动作。

调用方只有在 `ping.result.supported_plan_schema_versions` 包含 `4` 时才能发送该字段。v1-v3 的成功响应和默认成功行为保持不变；非 assert 失败响应的新增 `result.error`/已构造 rotation 仅是可选诊断字段，不改变顶层失败状态。
