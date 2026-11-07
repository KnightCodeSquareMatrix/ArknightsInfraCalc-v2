# 训练室输出与 plan.compute 协议

> 文档角色：active-change
> 生命周期状态：ready-on-request
> 当前真源：docs/接入与前端/FRONTEND_CLI.md
> 摘要：为训练室协助位增加结构化输出，并在不升级总体 plan schema 的前提下接入 Worker
> 所属工作流：docs/TODO/接口产品.md

## 1. 用户裁决

训练室一直存在。对于挂件体系设计，在没有训练任务的情况下也保留 1 个可用训练室挂件位；训练室技能角色优先于无技能角色。训练室实际求解与训练目标收益仍属于本任务后续实现范围。

## 2. 前端响应协议

`plan.compute` 成功结果增加可选字段 `training_room`。保持总体 `plan_schema_version: 3`，不新建请求 schema 版本；旧 Worker 不需要升级，也不应因缺少该字段而失败。

字段形状固定为：

```json
{
  "training_room": {
    "schema_version": 1,
    "shifts": [
      { "trainee": "干员名或 null", "trainer": "干员名或 null" }
    ]
  }
}
```

约束：

- `training_room` 在响应中可省略；省略表示当前 Worker 未提供训练室结果，不能解释为训练室为空。
- 出现时 `schema_version` 固定为 `1`。
- `shifts` 与 `result.rotation.shifts` 按位置对应，数量必须一致。
- `trainee` 是训练主位干员名；没有训练任务时为 `null`。
- `trainer` 是训练室协助位干员名；没有协助位时为 `null`。
- `trainee` 和 `trainer` 不得在同一 shift 重复。
- 本字段是响应增量，不改变 `result.schema_version`、`plan_schema_version` 或旧 Worker 请求校验。

## 3. 实现边界

后续实现需要：

1. 从同一轮 Plan / rotation 结果生成训练室输出，不启动第二套编排。
2. 保留训练室真实 `training_assist` 与训练技能数据的 owner 边界。
3. 在没有训练任务时仍允许一个策略性 trainer 挂件位，但不得伪造 `trainee`。
4. trainer 必须遵守所在班次的 presence、心情和其他体系挂件占用约束。
5. 当训练室角色同时服务其他 receiver 时，进入统一挂件容量裁决，不能由训练室后处理覆盖已有硬依赖。
6. MAA、legacy `plan` 和旧 Worker 不因该字段缺失或未实现而失败。

## 4. 非目标

- 本任务不升级 `plan_schema_version`。
- 本任务不修改 `contracts/plan-compute-v3.schema.json` 的请求结构。
- 本任务不在本轮实现完整训练目标求解、训练收益优化或训练室全局 objective。
- 本任务不把训练室挂件位扩展成多个协助位。

## 5. 验收标准

- v3 `plan.compute` 可在有训练室字段和无训练室字段两种响应下被前端兼容读取。
- `training_room.shifts` 与 rotation 班次数严格对应。
- 无训练任务时输出 `{ "trainee": null, "trainer": <name 或 null> }`，不产生虚构训练主位。
- 有训练任务时输出真实主位和协助位，空位使用 `null`。
- 旧 Worker / legacy `plan` 不要求该字段，不因字段缺失失败。
- 训练室 trainer 与其他 receiver 发生资源冲突时，统一容量裁决结果可解释且无 dangling dependency。
