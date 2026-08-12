# ADR 0006: 二班与深海轮换的缺员容错与终验契约

> 文档角色：decision
> 生命周期状态：accepted
> 当前真源：docs/定时换班.md；docs/SCHEDULE_ROTATION.md；docs/FRONTEND_CLI.md；docs/FRONTEND_SERVE_GUIDE.md
> 摘要：裁决 `main_backup_12_12` 与 `abyssal_7_5_7_5` 的主备求解、办公室与会客室缺员容错、绑定终验、warnings 输出及 ABC 全 profile 一致性

> 日期：2026-08-09
> 实现参考：`crates/infra-core/src/schedule/team_rotation.rs`；`crates/infra-core/src/schedule/base_rotation.rs`；`crates/infra-core/src/layout/orchestrate/plan.rs`；`crates/infra-core/src/layout/orchestrate/execute.rs`；`crates/infra-cli/src/commands/serve.rs`；`crates/infra-cli/src/output.rs`；`crates/infra-core/src/export/maa.rs`
> 历史评审：2026-08-09 前三轮定时换班方案复审（前两轮 REVISE，第三轮收敛）及用户逐项最终裁决

本 ADR 是实现完成后的对照检查基准，不声明任何代码修复或测试已经完成。当前领域语义仍由 canonical 文档定义；实现事实必须以代码、测试和可复现 evidence 为准。

## 背景与动机

当前办公室轮换路径把“每班必须满编”作为 hard error。`assign_rotating_offices` 在没有合法候选时于 `team_rotation.rs:605` 返回错误；最终轮换终验也在 `team_rotation.rs:4697-4705` 对指定设施要求实际人数必须等于容量。会客室则由基础编排 pipeline 的 `assign_meeting_rooms` 在 `pipeline.rs:252-261` 处理，轮换层另有 cohort 组装与补位逻辑。

这使二班和深海 profile 在办公室或会客室候选不足时无法产生“部分可执行、缺额可见”的结果，也使 ABC、菲亚四班与二班、深海的缺员语义不一致。当前 `TeamRotationReport` 在 `team_rotation.rs:230-246` 没有报告级 warnings；`RotationSummary` 在 `serve.rs:423-440` 也未透传该信息。文本输出仍在 `output.rs:1080` 使用“设施始终满编不空转”的绝对表述。

本 ADR 的目标是使以下不变量同时成立：

- 主 assignment 的选择优先于 backup assignment。
- 办公室和会客室候选不足时可留空，但缺额必须被结构化报告。
- 完整状态互斥 policy 不变。
- 绑定成员的实际 presence 必须与其唯一 Team 的 active 拓扑一致。
- 最终报告、文本输出和 serve response 必须忠实表达缺额。
- 不把局部 main 选择导致的 backup 无解扩大解释为整个 policy 无解。

## 决策

### D1. 所有 profile 统一采用“留空 + warnings”的办公室缺员语义

ABC 三班、二班、菲亚四班和深海四班的办公室候选不足时均：

1. 保留已经成功落位的办公室成员；
2. 未填满的席位保持空置；
3. 生成报告级 warning；
4. 不因办公室缺员直接返回 hard error。

菲亚四班复用 ABC，因此自动继承该语义。办公室缺员 warning 必须包含：

- `shift`；
- 设施类型；
- 房间 ID；
- 缺额数量。

办公室和会客室的文案必须可区分。

理由：办公室缺员属于结果质量下降，不等于整个排班不可执行。所有 profile 使用同一缺员契约，避免 ABC 与显式 profile 的行为分裂。

### D2. 显式二班/深海会客室保留 pipeline assignment，不重建 cohort；ABC 保留既有 cohort 轮换

二班（`main_backup_12_12`）与深海四班（`abyssal_7_5_7_5`）的会客室继续消费基础编排 pipeline 的 `assign_meeting_rooms` 结果，不重新构建会客室 cohort，也不将会客室成员纳入新的 Team 选型。ABC 三班保留既有 meeting cohort 轮换语义（已由 `abc_rotation_fills_meeting_room_with_rotating_meeting_skilled_cohort` 等测试固化），不在本次改动范围。

在显式二班/深海的每个最终 shift 中：

- 已由 pipeline 分配的会客室成员保持不变；
- 仅在存在合法空位候选时尽量补齐；
- 候选不足时保留空位并生成独立的会客室 warning；
- 会客室缺员不得转化为办公室 warning，也不得转化为 hard error。

理由：二班/深海是两套完整独立 assignment，会客室分配已经在完整班次上下文中完成求值，当前 pipeline 的责任边界位于 `pipeline.rs:271-382`。轮换层重建 cohort 会改变既有会客室结果，并可能使休息队成员因补位重新上岗。ABC 的 cohort 轮换是既有已验收功能，`rotation_shortfall_warnings` 同样覆盖 ABC 的会客室/办公室缺额，因此两类 profile 的缺员提示语义保持一致。

### D3. 二班办公室采用上 1 休 1，完整状态互斥 policy 保持不变

`main_backup_12_12` 的办公室成员按所属 owner team 上 1 休 1：

- main 状态只允许 main owner 的办公室成员上岗；
- backup 状态只允许 backup owner 的办公室成员上岗；
- 两个完整状态的所有成员集合保持互斥；
- 普通办公室成员不得因缺员补位而跨越完整状态互斥边界；
- 办公室不足时只留空并报告 warning。

绑定到跨设施体系的办公室成员仍必须随其消费方体系同上同下。完整状态互斥仍是 hard policy，不因办公室缺员而放宽。

理由：二班需要与 ABC 同级的轮换质量，但不改变已经确认的“完整状态互斥”责任边界。

### D4. 所有 profile 使用主 assignment 优先的求解与填充顺序

二班和深海均采用以下顺序：

```text
solve main
→ fill main
→ solve backup（排除 final main.operator_names()）
→ fill backup（forbidden = final main 全 assignment）
→ 深海额外执行 plan_abyssal_dorm_rest
```

其中：

- backup solver 使用最终 main 的 `operator_names()` 排除；
- backup fill 使用最终 main assignment 的全部成员作为 forbidden；
- main fill 后新增的办公室或其他成员也必须进入 backup 的排除边界；
- backup 无解时不得回到 main 重新选人，也不得重建会客室 cohort。

当前代码中的旧 backup 路径在 `team_rotation.rs:3684-3710` 仅基于 main assignment 做 solver 排除，且 `schedule_main_backup_12_12`、`schedule_abyssal_7_5_7_5` 分别在 `team_rotation.rs:3713-3772`、`4292-4376` 直接求 backup。实现后必须将填充阶段纳入该顺序。

理由：先完成 main 才能让 backup 看到最终资源占用；否则 main 的补位可能在 backup 阶段重新出现，破坏完整状态互斥。

### D5. fill helper 使用统一 forbidden 集合

所有轮换 fill helper 的 forbidden 集合必须为以下集合的并集：

```text
当前 assignment 已使用的成员
∪ 其他 owner 的全部成员
∪ 当前 owner plan.excluded_operators
∪ 当前 owner plan.rotation_reserves[].operators
```

字段来源：

- `AssignmentPlan.excluded_operators`：`crates/infra-core/src/layout/orchestrate/plan.rs:268-270`；
- `AssignmentPlan.rotation_reserves`：`plan.rs:284-286`；
- `execute_plan` 中对 `excluded_operators` 和 `rotation_reserves` 的 used 处理：`crates/infra-core/src/layout/orchestrate/execute.rs:83-88`。

`control_exclusions` 不得加入 fill helper 的 forbidden 集合。该字段只作用于中枢普通补位 admission，不能扩大为其他设施的全局排除。

理由：fill helper 必须消费与 `execute_plan` 一致的“已使用”语义；`control_exclusions` 与办公室、会客室候选资格属于不同责任边界。

### D6. 办公室 fill 支持有界候选回退，ABC 不增加连续工作 hard validation

对需要连续工作可行性约束的二班和深海办公室补位：

1. 按办公室评分从高到低尝试候选；
2. 对每个临时 assignment 复用连续工作可行性判定；
3. 候选不可持续时尝试下一个候选；
4. 所有候选均不可持续时留空并生成 warning。

已经被 Plan anchor 占用的办公室席位是 hard placement，不参与候选替换。

ABC 不增加连续工作 hard validation，因此不引入上述连续工作回退；ABC 候选不足时直接留空并生成 warning。

该回退是候选顺序上的有界 policy，不声称改变搜索为全局最优。现有办公室评分是静态 heuristic，见 `team_rotation.rs:481-531`。

理由：二班与深海有完整轮换连续工作终验，办公室补位必须避免产生随后必然失败的状态；ABC 当前验证覆盖不同，不应为支持回退而隐式扩大其 hard validation 范围。

### D7. 深海主备顺序与 dorm_rest 语义固定

`abyssal_7_5_7_5` 采用：

```text
solve main → fill main
→ solve backup（排除 final main）
→ fill backup（排除 final main 全 assignment）
→ plan_abyssal_dorm_rest
```

深海 `dorm_rest`：

- 继续在最终 main/backup assignment 上规划；
- 只为深海 5h 恢复态提供指定宿舍恢复组；
- 不重建会客室 cohort；
- 不把宿管的宿舍恢复元数据当作工作 presence；
- Shift 1 和 Shift 3 均必须独立生成并独立验证其 warning 与 presence。

深海 ShiftBind 的 `on:2, off:2` 在 `7/5/7/5` 拓扑下的工作向量是：

```text
[true, false, true, false]
```

`dorm_rest` 恢复 presence 不计入该工作拓扑。

理由：深海的 5h 状态是重复 owner 的两个独立输出状态，不能只验证一次或将 dorm_rest 误判为工作。

### D8. 终验以最终 shifts 为输入，缺员 warning 必须由纯函数生成

最终终验必须重新计算每个最终 shift 的办公室和会客室期望 warning，并与报告中的 `warnings` 精确比较。

终验要求：

- 办公室实际人数不得大于容量；
- 会客室实际人数不得大于容量；
- 低于容量时，缺额必须精确对应 warning；
- warning 比较不得解析自由文本；
- warning 必须由统一纯函数根据最终 shifts 生成；
- roster 足够时，所有办公室和会客室必须满编，且 warnings 为空；
- roster 不足时，允许最终 assignment 留空，但缺额和 warning 必须一致；
- 深海重复 owner 的 Shift 1 与 Shift 3 必须分别列出缺员 warning。

该终验只针对办公室和会客室放宽“等于容量”的要求；生产设施、中枢、Plan anchor、owner dependency 等已有 hard invariant 继续保持。

理由：最终状态可能经过填充、替换和 `dorm_rest` 处理，不能用中间候选或自由文本推断最终缺额。

### D9. 绑定终验比较 Team 拓扑，不套用 RuntimeShiftBind 数值

绑定终验必须：

1. 收集所有 profile assignment plans 产生的 bind；
2. 对每个 bind 查找包含其全部成员的 Team；
3. 要求恰好一个 Team 完整包含该 bind；
4. 零个 Team 或多个 Team 完整包含该 bind 均报错；
5. 对每个成员比较其实际 presence vector 与该唯一 Team 的 active 拓扑 vector。

不得直接使用 `RuntimeShiftBind.on_shifts` / `off_shifts` 的数值作为 expected vector。该数值是绑定元数据；实际期望由 profile 的 shift 拓扑决定。`shift_bind.rs:14-19` 当前定义了该元数据，`team_rotation.rs:4763-4787` 当前以唯一 Team 的 active 拓扑进行比较，但对找不到 Team 的情况静默 `continue`，实现后必须改为错误。

理由：不同 profile 的班次数量和时长不同，`on/off` 数值不能替代实际 presence 拓扑。跨 owner bind 说明 Team 编制或 owner 绑定边界错误，不能静默跳过。

### D10. TeamLabel 唯一性只覆盖普通 assignment 成员

`dorm_rest` 元数据不得新增 `TeamLabel`。

- 如果宿管本来就是某个 owner 的普通 assignment 成员，保留其既有唯一 TeamLabel；
- 仅由 `dorm_rest` 引入、且不属于普通 assignment 的成员允许没有 TeamLabel；
- TeamLabel 唯一性校验只覆盖普通 assignment 成员；
- `dorm_rest` 引用不能反向改变 Team 成员集合或 active 拓扑。

理由：宿舍恢复是执行元数据，不是新增工作队伍。将临时宿管强行加入 Team 会污染 presence 验证和轮休语义。

### D11. TeamRotationReport 与各输出层透传 warnings

`TeamRotationReport` 增加：

```rust
pub warnings: Vec<String>
```

并使用 `skip_serializing_if = "Vec::is_empty"`。

所有构造 `TeamRotationReport` 的字面量必须补充空值或实际 warning，至少包括：

- `team_rotation.rs:3617` 的 ABC 构造；
- `team_rotation.rs:4579` 的 finalizer；
- `crates/infra-cli/src/commands/box_profile/eval.rs:292`；
- `crates/infra-core/src/export/maa.rs:912` 的测试 fixture。

`RotationSummary` 在 `serve.rs:423-462` 必须透传 warnings。旧 `PlanResult` 在 `serve.rs:395-412` 必须明确其契约：

- 若该 legacy 输出仍属于当前支持的输出，则增加 `warnings` 字段并透传；
- 若保持 legacy 不提供 warnings，则必须在代码和相关文档中明确这是有意的 legacy 限制，并有测试证明 `plan.compute` 的 `RotationSummary` 仍提供完整 warnings。

理由：报告级 warning 只有在 core、serve JSON、legacy 输出和文本输出之间保持一致时，前端和人工执行者才能正确识别缺额。

### D12. 文本输出必须准确描述可能空置，并增加 warnings 段

`output.rs:1080` 的“设施始终满编不空转”必须改为准确表述，不能继续暗示办公室和会客室永远满编。

文本输出必须增加独立 warnings 段：

- warnings 为空时可省略该段；
- warnings 非空时逐条打印；
- 文案至少能区分办公室与会客室；
- 每条包含 shift、设施、room id 和缺额；
- 不把 warning 打印为 hard error；
- 不改变现有排班、设施和效率输出含义。

理由：文本输出是人工换班的直接检查界面，必须反映“可执行但有缺额”的状态。

## 后果

### 实现影响

预期修改范围：

- `crates/infra-core/src/schedule/team_rotation.rs`
  - 主备求解与 fill 顺序；
  - office/meeting 缺员处理；
  - forbidden 集合；
  - office 候选回退；
  - final warnings 纯函数；
  - bind 与 TeamLabel 终验；
  - `TeamRotationReport` 字段。
- `crates/infra-core/src/schedule/base_rotation.rs`
  - 仅在需要复用或暴露连续工作可行性判定时修改；
  - 不改变效率计算的责任边界。
- `crates/infra-cli/src/commands/serve.rs`
  - `RotationSummary` 和必要的 legacy `PlanResult` warnings 契约；
  - 不修改 `plan-compute-v1` 请求解析语义。
- `crates/infra-cli/src/output.rs`
  - 文本 warnings 段和准确的设施满编表述。
- `crates/infra-core/src/export/maa.rs`
  - 必要的测试 fixture 或注释更新；
  - MAA export 不负责重新选择办公室、会客室或 Team。
- `docs/定时换班.md`
  - 删除 ABC 办公室必满编的绝对表述；
  - 补充缺员 warning、二班、深海和统一 profile 语义。
- `docs/SCHEDULE_ROTATION.md`
  - 同步当前实现覆盖和失败/缺员语义。
- `docs/FRONTEND_CLI.md`
  - 记录 response 中 warnings 的含义和 legacy 输出边界。
- `docs/FRONTEND_SERVE_GUIDE.md`
  - 记录 `rotation.warnings` 的透传契约。
- 新 ADR 文件。

不修改：

- Mower；
- 会客室 cohort 设计；
- `contracts/plan-compute-v1.schema.json`；
- “完整状态互斥” policy；
- CLI profile 名称或既有 profile 选择语义。

### 文档影响

当前 canonical 中以下表述必须同步修正：

- `docs/定时换班.md:312-314` 的“每班办公室必须满编”；
- `docs/SCHEDULE_ROTATION.md:37-38` 的“每班满编”和“禁止设施空转”。

文档应明确：

- “办公室和会客室候选不足时留空并报告 warning”；
- “实际人数不得超过容量”仍是 hard invariant；
- “roster 充足时必须满编且 warnings 为空”；
- warnings 是结果报告，不是允许其他设施违反 hard constraint 的通行证；
- `plan-compute-v1.schema.json` 只约束请求，不因 response 增加 warnings 而修改。

ADR 不取代 canonical 文档；实现完成后 canonical 必须与本 ADR 的已确认语义一致。

### 测试影响

至少增加或更新以下测试：

- ABC、二班、菲亚四班、深海四班在办公室候选不足时成功返回，办公室保留空位并生成 warning；
- 菲亚四班复用 ABC 缺员语义；
- 会客室候选不足时保留 pipeline assignment，补足可用候选，剩余空位生成独立 warning；
- 会客室轮换不重建 cohort，不引入休息队成员；
- 主 assignment 填充后，backup solver 排除 final main 全成员；
- backup fill 使用相同 forbidden 边界；
- `excluded_operators` 和 `rotation_reserves` 生效，`control_exclusions` 不影响 office/meeting fill；
- office 候选按评分顺序回退，并对每次临时 assignment 运行连续工作可行性判定；
- anchor 已占用办公室不会被回退替换；
- ABC 不因连续工作回退而新增 hard validation；
- actual office/meeting 人数大于容量时终验失败；
- 缺额与纯函数生成的 warnings 精确一致；
- 充足 roster 下办公室和会客室满编且 warnings 为空；
- 深海 Shift 1、Shift 3 的重复 owner 缺额分别生成 warning；
- ABC、二班和深海的 bind presence 与 Team active topology 一致；
- bind 找不到唯一完整 Team 或被多个 Team 完整包含时终验失败；
- 深海 `7/5/7/5` 的 bind presence 为 `[true, false, true, false]`；
- `dorm_rest` 不计入工作拓扑；
- dorm_rest 新增宿管不产生 TeamLabel，既有 owner 成员保留唯一标签；
- serve `RotationSummary` 透传 warnings；
- legacy `PlanResult` 按已选契约测试“提供 warnings”或“明确不提供”；
- 文本输出包含 warnings 段且满编表述准确；
- 所有现有 `TeamRotationReport` fixture 补齐 warnings；
- 请求 schema 文件未修改，schema 测试和契约 hash 保持有效；
- 至少一次真实 `plan` 或 `layout team-rotation` 运行，核对 core、serve 和 MAA 输出一致。

## 验收清单

### 代码事实

- [ ] `TeamRotationReport` 存在 `warnings: Vec<String>`，为空时不序列化。
- [ ] ABC、二班、菲亚四班、深海四班的办公室缺员均为留空加 warning，不再因办公室不足 hard error。
- [ ] 会客室保留 pipeline `assign_meeting_rooms` 结果，不重建 cohort。
- [ ] 二班和深海遵循 `solve main → fill main → solve backup → fill backup` 顺序。
- [ ] backup solver 排除 final main 的 `operator_names()`，backup fill 排除 final main 全 assignment。
- [ ] fill forbidden 集合包含当前 assignment、其他 owner 成员、`excluded_operators` 和 `rotation_reserves`，不包含 `control_exclusions`。
- [ ] office 回退按评分尝试并复用连续工作判定；ABC 不新增该 hard validation。
- [ ] Plan anchor 办公室 placement 不参与替换。
- [ ] finalizer 对 office/meeting 执行 actual `> capacity` hard rejection。
- [ ] bind 缺少唯一 Team 或被多个 Team 完整包含时返回错误，不再静默 `continue`。
- [ ] bind 终验比较 Team active topology vector 与实际 presence vector，不直接套用 `on/off` 数值。
- [ ] `dorm_rest` 不新增 TeamLabel，且不进入工作拓扑。
- [ ] 主选择导致 backup 无解时，错误明确限定为“当前 main 选择下无解”。

### 终验行为

- [ ] 充足 roster 的四种 profile 均达到办公室和会客室满编，warnings 为空。
- [ ] 办公室不足的最终 shifts 可返回，缺额与 office warnings 一一对应。
- [ ] 会客室不足的最终 shifts 可返回，缺额与 meeting warnings 一一对应。
- [ ] 深海 Shift 1 与 Shift 3 的重复 owner 缺额分别可观察。
- [ ] 二班办公室符合上 1 休 1，且完整状态成员集合互斥。
- [ ] 深海 `7/5/7/5` 下 bind presence 为 `[true, false, true, false]`。
- [ ] dorm_rest 恢复成员不会导致 active presence 被判定为工作。
- [ ] 任何 office/meeting actual 超容量的构造都被拒绝。

### 输出契约

- [ ] `RotationSummary.warnings` 与 `TeamRotationReport.warnings` 一致。
- [ ] legacy `PlanResult` 的 warnings 行为已明确并有测试覆盖。
- [ ] warning 文案包含 shift、设施、room id 和缺额。
- [ ] office 与 meeting warning 可区分。
- [ ] 文本输出增加 warnings 段。
- [ ] 文本输出不再声称所有设施始终满编。
- [ ] MAA export 只忠实输出排班动作，不重新选择候选或改变 warnings 语义。
- [ ] `plan-compute-v1.schema.json` 未修改。

### 测试用例

- [ ] ABC 缺办公室候选。
- [ ] 二班缺办公室候选并验证主备互斥。
- [ ] 深海缺办公室候选并验证 Shift 1/3 独立 warning。
- [ ] 菲亚四班继承 ABC 缺员行为。
- [ ] 会客室缺员且 pipeline 已有部分 assignment。
- [ ] office 候选连续工作不可行后成功回退到下一候选。
- [ ] office anchor 不被回退替换。
- [ ] bind 零 Team、多 Team 和唯一 Team 三种终验路径。
- [ ] dorm_rest 引入成员的 TeamLabel 和 presence 行为。
- [ ] serve JSON、legacy JSON、文本输出和 MAA 输出回归。

### 证据

- [ ] 通过统一 evidence 工具记录 `cargo fmt --all -- --check` 结果。
- [ ] 通过统一 evidence 工具记录 infra-core 相关单元测试结果。
- [ ] 通过统一 evidence 工具记录 infra-cli、serve 和输出测试结果。
- [ ] 通过统一 evidence 工具记录四种 profile 的真实 CLI 或 `plan` 运行结果。
- [ ] 通过统一 evidence 工具记录请求 schema 未修改及契约 hash 验证。
- [ ] evidence 中保存缺员、满编、bind、backup 无解和 warnings 精确匹配的实际输出。
- [ ] evidence 中明确列出未运行的测试类别和未验证环境。

## 保留限制 / 非目标

- 本 ADR 不声明实现已经完成，也不把当前文档裁决当作测试通过证明。
- “留空 + warning”只适用于办公室和会客室候选不足；其他已有 hard constraint 不因此放宽。
- 实际人数超过办公室或会客室容量始终是错误。
- warnings 不保证排班达到全局最优，只说明缺额被准确报告。
- office 静态评分和候选回退是 policy / heuristic，不构成全局最优证明。
- main 补位可能使 backup 无解；错误只能描述为当前 main 选择下无解，不能宣称完整 policy 整体无解。
- 完整状态互斥 policy 不变。
- 显式二班/深海不重建会客室 cohort；ABC 保留既有 cohort 轮换（见 D2）。
- **终验覆盖差异**：warnings 纯函数统一生成并用于全部 profile 的报告与输出；二班、深海与菲亚四班经 `validate_profile_rotation` 做 warnings 重算比对、office/meeting 超容量拒绝、bind 唯一 Team 与 is_two_team 的 TeamLabel 唯一性终验；ABC 走既有 `validate_final_rotation_invariants` 路径，warnings 在报告构造时由同一纯函数生成，未再独立重算比对，也不套用 bind 唯一 Team / TeamLabel 唯一性终验。该差异是既有 ABC 验证路径的延续，不属于本 ADR 引入的回退。
- 不把 `RuntimeShiftBind.on_shifts` / `off_shifts` 解释为跨 profile 的实际工作拓扑。
- 不把 dorm_rest 恢复成员加入新的 Team。
- 不修改 `plan-compute-v1` 请求 schema。
- 不负责 Mower 的动态换班、动态替补或执行器行为。
- 不实现更长周期的办公室个人长期上二休一模型。
- 不改变菲亚梅塔、无人机和其他既有 schedule/export 责任边界。

## 已放弃的替代方案

### 办公室或会客室候选不足直接报错

放弃。候选不足是可报告的输出质量问题；只要其他 hard invariant 成立，排班仍可供人工或执行器处理。缺额必须通过 warnings 显式暴露。

### 为显式二班/深海重新构建会客室 cohort

放弃。二班/深海的会客室结果由基础 pipeline 的 `assign_meeting_rooms` 负责。轮换层重建 cohort 会改变已有 assignment，并可能使休息队成员越过 active admission 边界。ABC 的既有 cohort 轮换不属于本 ADR 范围，保持不变（见 D2）。

### 先求解 backup，再对两个状态统一补位

放弃。该顺序无法保证 main 补位后的成员不会进入 backup。最终 main 必须先完成，backup 才能按完整 main assignment 建立排除边界。

### 用 `control_exclusions` 作为所有设施的全局 forbidden

放弃。`control_exclusions` 只属于中枢普通补位 admission，不能代替 required admission 或改变办公室、会客室候选语义。

### 用 `on_shifts/off_shifts` 数值直接生成 bind expected vector

放弃。ABC、二班和深海的班次数量及拓扑不同。expected vector 必须来自唯一 Team 的 active topology。

### bind 找不到 Team 时静默跳过

放弃。零个或多个完整 Team 都表示 ownership 或编制错误，必须由终验报告。

### 把 dorm_rest 宿管加入 TeamLabel 编制

放弃。宿舍恢复元数据不是工作队伍；仅由 dorm_rest 引入的成员允许零标签。

### 让 ABC 也增加连续工作 hard validation 和候选回退

放弃。ABC 当前只提供既定验证范围。为支持回退而扩大 ABC hard validation 会改变既有 profile 语义。

### 用自由文本解析 warnings 进行终验

放弃。warning 文案不是机器契约。终验必须从最终 shifts 通过统一纯函数重新计算期望 warnings 并比较结构化结果。

### 修改 `plan-compute-v1.schema.json` 以容纳 response warnings

放弃。该 schema 只约束请求参数，response 的 `RotationSummary` 由 serve 输出结构负责。请求 schema 不因本 ADR 修改。

### 把当前 main 选择导致的 backup 无解描述为 policy 整体无解

放弃。二班和深海采用的是当前 main 选择下的互斥 backup policy；错误范围必须保持准确，不能扩大结论。

### 让 Mower 负责补足 InfraCalc 的轮换缺员

放弃。Mower 不在本次范围内。InfraCalc 必须先输出准确的最终 assignment 与 warnings，执行器只消费已确定的排班结果。
