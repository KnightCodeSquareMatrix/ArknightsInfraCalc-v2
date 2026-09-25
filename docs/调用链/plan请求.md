# `plan` 请求调用链

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/接口/CLI与Worker.md；docs/接口/输入输出与兼容.md；`commands/plan_compute.rs`
> 摘要：沿一次 plan/plan.compute 请求定位入口、状态转换、共享求解、profile、MAA 和 advice
> 代码范围：`crates/infra-cli/src/commands/{plan.rs,serve.rs,plan_compute.rs}`、`crates/infra-core/src/{box_profile,schedule,layout,export,training_advice}`
> 代码锚点：`main::run` → `plan_cmd`/`handle_request` → `compute_plan`/`compute_plan_for_plan_compute` → `compute_plan_inner`
> 关联链路：[CLI 与 Worker](../接口/CLI与Worker.md)、[输入输出与兼容](../接口/输入输出与兼容.md)、[debug 与验证](debug与验证.md)
> 核验基线：标准 243 layout + full-e2 operbox；Worker `ping` 与 `plan.compute`；`--assert-invariants`

## 两个入口汇合

```text
argv plan ─────────────┐
serve method=plan ────┼→ compute_plan → compute_plan_inner
serve plan.compute ───┘                 │
                                        ├→ current probe / rotation
                                        ├→ optional BoxProfile
                                        └→ optional MaaSchedule
```

`plan` 文件入口和 Worker `method=plan` 都调用 `compute_plan`；`plan.compute` 使用 `compute_plan_for_plan_compute` 保留 partial result。三者只负责适配输入和输出，不各自实现排班。

## 阶段一：适配与加载

- CLI：`commands/plan.rs::plan_cmd` 解析 `--layout`、`--operbox`、`--baseline`、`--rotation`、`--top`、`--prefer`、输出路径和 `--assert-invariants`，加载 `BaseBlueprint`、`OperBox`、`OperatorInstances`、`SkillTable`。
- Worker `plan.compute`：`serve::handle_request` 先解析 envelope；`handle_plan_compute` 读取 `schema_version`，选择嵌入 schema，执行 schema 校验、布局语义校验和 `inline_operbox`。
- Worker `plan`：`handle_plan` 反序列化路径参数，默认加载 243/full-e2，调用同一 `compute_plan`。

错误在此阶段属于协议、schema、输入或资源加载错误，不应进入 solver 根因分析。

## 阶段二：共享 Plan 编排

`plan_compute::compute_plan_inner` 首先调用：

```text
run_user_rotation_probe_with_profile_and_preferences(
  blueprint, operbox, instances, skill_table,
  top_k, rotation_profile, system_preferences,
  fiammetta_enable, optional candidate output
)
```

该 probe 进入 `schedule_timed_rotation`，由 rotation profile 分派单班编排和多班轮换。Peak/shift assignment、Plan、resolve、房间搜索、跨设施状态和最终 rotation 都属于 `infra-core`；CLI 只接收 `LayoutProbe`。

若 probe 失败，`PlanComputeFailure` 可能带 partial rotation；`plan.compute` 在未启用断言时会序列化 partial `rotation` 与 `error`，CLI `plan` 则返回失败。

## 阶段三：可选断言、profile 与 MAA

1. `assert_invariants=true` 时，对当前 rotation 调用 `assert_final_rotation_invariants`。失败立即停止，不继续 profile/MAA。
2. 请求 profile 时，`build_box_profile_from_current_probe` 消费同一个 current probe，避免为 profile 再跑一次用户 rotation；baseline 比较在 profile 阶段独立读取。
3. 请求 MAA 时，`build_from_team_rotation` 把同一 `current.rotation` 映射为 `MaaSchedule`，不重新选人。
4. `plan.compute` 额外调用 `build_plan_compute_training_advice`，使用 inline layout、operbox、知识文件和当前 rotation 的平均效率上下文；这是输出建议投影，不回写求解。

## 输出投影

- `plan_cmd` 写 profile、可选 rotation/assignment/MAA 文件，并把 profile 与 rotation 文本打印到 stdout。
- `handle_plan` 返回 `PlanResult`，可同时写文件。
- `handle_plan_compute` 返回 `PlanComputeResult`，外层 `ServeResponse` 再包装 id、耗时、solver build info 和 error。

看到“profile 与 MAA 不一致”时，先确认二者来自同一请求的 `current.rotation`，再分别检查 `BoxProfile` 投影和 `export/maa.rs`；不要先改 solver。

## 症状到 owner

| 症状 | 先查 |
|---|---|
| 请求被拒绝 | schema、`BaseBlueprint::validate`、`inline_operbox` |
| 轮换无解或候选缺失 | `schedule/team_rotation.rs`、`layout/assign*`、`layout/orchestrate/*` |
| 固定编制效率错误 | `layout/resolve.rs` → 对应设施 solver；用 `layout eval --explain-json` |
| profile 画像错误 | `box_profile/build.rs` 和 `training_advice`，确认 current probe 是否正确 |
| MAA 房间/班次字段错误 | `export/maa.rs`，确认 rotation assignment 已正确 |
| Worker 与 CLI 行为不一致 | `plan_compute.rs` 共享入口、各自 adapter 的默认值和输出投影 |
