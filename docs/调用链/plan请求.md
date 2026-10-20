# `plan` 请求调用链

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/接口/CLI与Worker.md; docs/接口/输入输出与兼容.md
> 机制依赖：docs/知识库引用.md；vault:docs/4-排班算法/可露希尔基建终端排班机制.md；vault:docs/5-换班操作/定时换班.md
> 摘要：沿一次 plan/plan.compute 请求定位入口、状态转换、共享求解、profile、MAA 和 advice
> 代码范围：`crates/infra-cli/src/commands/{plan.rs,serve.rs,plan_compute.rs}`、`crates/infra-core/src/{box_profile,schedule,layout,export,training_advice}`
> 代码锚点：`crates/infra-cli/src/main.rs::run` → `crates/infra-cli/src/commands/plan.rs::plan_cmd` / `crates/infra-cli/src/commands/serve.rs::handle_request` → `crates/infra-cli/src/commands/plan_compute.rs::compute_plan` / `crates/infra-cli/src/commands/plan_compute.rs::compute_plan_for_plan_compute` → `crates/infra-cli/src/commands/plan_compute.rs::compute_plan_inner`
> 关联链路：`docs/接口/CLI与Worker.md`; `docs/接口/输入输出与兼容.md`; `docs/调用链/debug与验证.md`
> 核验基线：标准 243 layout + full-e2 operbox；Worker `ping` 与 `plan.compute`；`--assert-invariants`；贸易最终效率续接 `ddf5218`/`3f1d92e`/`01cbec5`

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
- Worker `plan.compute`：`serve::handle_request` 先解析 envelope；`handle_plan_compute` 读取 `schema_version`，选择 v1–v6 嵌入 schema。旧 v3–v5 body 若请求已删除的独立龙巫 profile，会在进入求解前明确失败；v6 schema 不包含该枚举。其余请求继续执行 schema 校验、布局语义校验和 `inline_operbox`。
- Worker `plan`：`handle_plan` 在反序列化路径参数前拒绝旧龙巫 wire；其他请求默认加载 243/full-e2，调用同一 `compute_plan`。

布局语义校验拒绝超出 1–5 级的宿舍 `level`。旧 `dorm_ambience_level` 可读取但不参与结算；`dorm_beds` 仅负责容量；旧 `scenario.initial_global.monster_cuisine` 可读取但会从初始资源池忽略。CLI 文件加载与 Worker 内联布局使用同一 `BaseBlueprint::validate`，不会把这些遗留字段当作当前料理来源。其他真正非法输入仍在此阶段返回，不应进入 solver 根因分析。

## 阶段二：共享 Plan 编排

`plan_compute::compute_plan_inner` 首先调用：

```text
run_user_rotation_probe_with_profile_and_preferences(
  blueprint, operbox, instances, skill_table,
  top_k, rotation_profile, system_preferences,
   fiammetta_enable, support_combinations_enable, optional candidate output
)
```

该 probe 进入 `schedule_timed_rotation`，由 rotation profile 分派单班编排和多班轮换。Peak/shift assignment、Plan、resolve、房间搜索、跨设施状态和最终 rotation 都属于 `infra-core`；CLI 只接收 `LayoutProbe`。

主动挂件开关从请求经 probe、`AssignBaseOptions` 进入 Plan 编译，缺省关闭。规则中 `proactive_support=true` 的角色在关闭时不参与落位；主角色和普通角色保持原有竞争，未选中的支持角色不产生 active dependency。开启时不改变当前规则。最终 `support_combination_policy` 断言按 Plan 认领及当班 assignment 检查，允许自然在岗读数，不按姓名或 tag 拒绝成员。该 policy 不序列化为输出字段。

`pozyomka_durin` 的已确认核心条件是：四条真实赤金制造线、三名核心均精二、四名杜林可在实际基建中兑现，并且恰为两座或三座赤金金单贸易站。恰有两座时，绮良独占一站，鸿雪与图耶同站；恰有三座时三人各占一站，绮良优先与巫恋/龙舌兰组队，鸿雪优先与可露希尔组队，图耶优先与但书组队。四名队友只是优先建议，不是核心准入条件，也不是必需 anchor；候选缺失或其固定填房失败时，Plan 重编译并让该人回普通 Trade 搜索。若三人核心缺练度、四杜林无法兑现、布局不是上述两种贸易站数量，或完整核心填房/中枢准入不可行，则不留下半套核心，改由普通 `trade_core` / Trade 搜索继续排班。旧同房 `single_trade_gold_flow` 路径已按用户要求由跨站模板替换；这不禁止普通搜索按技能规则得到同房结果。

核心三人形成自己的同步轮休绑定，不让建议队友合并改变核心轮休队形。ABC 三班按两上休一核对；其他 profile 按各自团队拓扑核对同上同下，不把三班 2/1 次数套到主备两班。门禁按最终非宿舍工作岗位计数：核心 team 的休班上三人必须全不在工作岗位，落在宿舍属于休息；实际工作班分别满足相应双站/三站几何，并至少有一个完整工作班证明核心实际生效。普通入口保留排班和可定位质量 warning，显式 `--assert-invariants` 才对最终真实拆组、班数或几何错误失败。

**尚未实现的择优**：用户 10-20 已明确比较字段为 `TradeEfficiency::final_efficiency`：技能效率计入纸面总效率，再乘特殊订单单位产出倍率；`mechanic_equivalent_efficiency` 仅解释机制，不重复计入。单房 `search_trade_triples`、`role_pick` 和贸易补入比较已在 `ddf5218` 改为读取 `final_efficiency`；`Stations` 仍是独立的站数加权复合。当前 Plan 阶段仍按优先级与 exclusive group 提交首个可行候选，没有跨完整候选评分器；单个候选内的排序不能证明两套完整 Plan 已比较。排班周期汇总/跨域 weighted policy 仍读纸面口径，四赤金对普通完整 Plan 的同场比较尚未实现，因此不声称已实现该择优或全局最优。

Rosemary alternative 要求候选 Plan 真实感知至少 60；50–59 时它不入选，技能本身仍按实际数值结算。`--prefer rosemary_perception=recruit_refresh_witch` 已是未知 alternative，不能再解除任何贸易禁配；黑键—巫恋同站限制由贸易搜索执行，不依赖该规则是否激活。

自动化组进入 Peak Plan 时，单班 owner 从同一种子分别完成带自动化锚点与跳过自动化组的普通方案。后者同时排除承曦格雷伊的发电占位；双方固定同一蓝图和配方，完成全部制造站填房（三级站须满员）并刷新实际效率后比较全部制造房的直接效率。普通方案严格胜出才替换整份 Plan/assignment；平手保留自动化，轮换只消费所选结果。比较失败返回错误，不能把未结算分支当作低效率分支。

自动化组合与普通组合的整班择优已移除，留待 DFS 阶段；当前不再输出 `[automation-branch]`。

若 probe 失败，`PlanComputeFailure` 可能带 partial rotation；`plan.compute` 在未启用断言时会序列化 partial `rotation` 与 `error`，CLI `plan` 则返回失败。

## 阶段三：可选断言、profile 与 MAA

1. `assert_invariants=true` 时，对当前 rotation 调用 `assert_final_rotation_invariants`，按实际制造技能检查清零组合的同房资格与制造房容量，并检查各班贸易房黑键—巫恋禁同站、杜林在岗人数、跨贸易站产线来源、读取者效率、已选鸿雪杜林核心的四条真实赤金线门槛及已选拆站方案的最终站位。失败立即停止，不继续 profile/MAA。
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
