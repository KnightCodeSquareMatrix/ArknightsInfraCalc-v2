# ADR 0008: 办公室连续工作时长预算与通用轮换准入

> 文档角色：decision
> 生命周期状态：proposed
> 当前真源：docs/定时换班.md；docs/SCHEDULE_ROTATION.md；docs/排班模式.md
> 摘要：规定办公室轮换按实际心情消耗计算连续工作时长，不按 profile 或干员姓名写死换班规则

> 日期：2026-08-20
> 实现参考：`crates/infra-core/src/schedule/team_rotation.rs`；`crates/infra-core/src/mood/eta.rs`；`crates/infra-core/src/mood/drain.rs`
> 评审状态：三轮独立只读复审通过（APPROVED）；本 ADR 不声明代码已实现

## 背景

办公室当前同时存在三类互相冲突的表达：

- ABC 办公室按队伍 cohort 轮换；
- 为减少日内操作，普通办公室成员需要在相邻班次连续工作；
- 斥罪等高消耗干员不能依靠姓名特判或固定班次下标解决。

如果在 schedule 中写死 `S2/S3`、`index == 2` 或某个 profile 的时长，未来自定义班次无法复用；如果只保存普通成员姓名集合，又无法保证多办公室的 `room_id -> operator` 稳定映射。

## 决策

### D1. 连续工作约束以最终班次时长和实际净消耗为输入

办公室候选准入不识别 `S1`、`S2`、`S3`，不识别 `12/6/6`、`8/8/8` 等 profile 名称，也不按干员姓名写特殊分支。

对每个办公室干员，在最终班次序列中计算连续工作段：

```text
若本班在办公室：
    continuous_hours += shift.duration_hours
    continuous_cost += drain_per_hour(final_room_context) * shift.duration_hours
若本班不在办公室：
    continuous_hours = 0
    continuous_cost = 0
```

候选加入当前办公室班次前，必须满足：

```text
continuous_cost + current_shift_cost <= workable_mood
```

其中 `drain_per_hour` 必须来自现有 mood owner 的最终房间上下文计算，不由办公室 schedule 重复实现公式。`workable_mood` 的数值以用户当前裁决和 mood canonical/data 真源统一后的值为准；本 ADR 不新增第二套心情常量。

数值口径已确定：全局满心情上限为 `24`，`data/mood_model.json` 和 `MoodModel` 的 `base_drain_per_hour=1.0` 保持不变；办公室在中枢满 5 人时的基础净消耗为 `1.0 - 0.25 = 0.75/h`。办公室连续预算必须消费最终房间上下文计算出的净消耗，不能把 `0.75/h` 作为绕过中枢、房间技能或干员自身修正的办公室专用常量。

现有 `0.5h` 安全余量也必须纳入同一预算 helper。候选准入和最终终验不得一个使用裸 `workable_mood`、另一个使用安全余量。

共享 helper 对连续工作段的每个前缀使用同一不等式：

```text
prefix_cost + drain_per_hour(prefix_end_context) * safety_margin
    <= workable_mood
```

其中 `prefix_cost` 是该连续段从起点到当前班次末尾的实际逐班成本总和，`drain_per_hour(prefix_end_context)` 是当前候选在该班次最终房间上下文中的净消耗；不得只取首班、末班或全段最大值作为未声明的替代 policy。若候选改变了中枢或其他全局上下文，必须在事务性最终 assignment 上重新计算该班次 drain。

### D2. 候选准入和最终终验使用同一纯函数

连续工作判断必须有单一 owner，建议在 `schedule/team_rotation.rs` 提取纯函数，至少消费：

- `BaseBlueprint`；
- 最终 `TeamShiftResult.assignment`；
- 每个 `shift.duration_hours`；
- `OperatorInstances`、`SkillTable` 和 `MoodModel`；
- profile 的最终班次顺序。

候选搜索阶段可以使用该函数做安全拒绝；最终 report 生成后必须再次对最终 assignments 计算，防止菲亚后处理、绑定填充或其他替换使中间结果失真。

候选尝试必须是事务性的：临时加入候选，完成该 shift 的绑定、其他设施补位和必要后处理，使用最终房间上下文计算预算；候选失败时回滚整个临时 assignment、used 集合、Team/presence 事实和效率结果，再尝试下一候选。不得只用办公室 partial assignment 计算后继续提交中枢或其他设施。

该约束是 **hard constraint**，不是排序 heuristic。候选被拒绝只表示该候选在当前已确定上下文和时长下不可行，不表示整个账号或 profile 全局无解。

### D3. 普通办公室成员是唯一允许跨 cohort 连续复用的例外

为实现日内低频换班：

- 普通办公室成员可以在连续工作段内跨越原 αβγ active cohort 继续工作；
- 该例外只作用于办公室普通成员；
- 生产、中枢、会客室、宿舍和其他设施不继承该例外；
- 跨设施绑定办公室成员仍必须随消费方的 `shift_bind` 同上同下；
- 固定复用状态必须保存 `room_id -> Vec<AssignedOperator>`，不能只保存姓名集合；当前办公室容量为 1，但状态结构不应把该事实硬编码进通用连续预算 helper。

普通办公室 admission 与绑定办公室 admission 必须分开：

- 普通成员可以进入连续预算候选，并获得跨 cohort 例外；
- 绑定成员只能由最终 Plan 的 resolved dependency 和消费方 presence 驱动；
- 绑定成员缺席时留空或报既有 hard failure，不得由普通办公室 fallback 代替；
- 绑定最终仍须通过唯一 Team 和 active topology 终验，沿用当前排班 canonical 的责任边界。

普通成员一旦离开办公室，连续工作段归零；之后重新进办公室须重新经过连续预算检查。

连续段的房间映射也是最终 hard invariant：被复用的普通成员必须仍位于映射记录的原 `room_id`；映射中的房间必须仍是办公室且容量足够。候选阶段发现房间缺失、容量冲突或后处理改房时，必须回滚并重试候选；最终终验发现映射与最终 assignment 不一致时必须返回 hard error，不得静默换房来修复。

### D4. 高消耗干员不使用姓名特判

斥罪当前高消耗行为必须由 mood 数据和最终房间上下文产生更高的 `drain_per_hour`，由 D1 自动限制其连续工作时长。

不得新增：

- `name == "斥罪"` 的 schedule 分支；
- 针对 `S1/S2/S3` 的斥罪条件；
- 针对某个 profile 的斥罪例外。

如果现有 mood 数据不能表达用户裁决的基础消耗、满心情或斥罪办公室修正，先修复 mood canonical/data owner，再让 schedule 消费统一结果。

### D5. 跨周期边界必须闭环验证

最终终验不能只从第一个班次开始线性扫描。对每名办公室干员，至少应从每个 shift 作为起点扫描一整轮，或等价地把最终 shift 序列循环展开一次以上，以覆盖：

```text
最后一个班次工作 + 第一个班次继续工作
```

连续工作段的边界只由“是否在办公室”决定，不由 shift 下标、TeamLabel 或 profile 名称决定。

若干员在完整周期的每个 shift 都在办公室，且该周期累计净消耗大于零，则没有可证明的恢复边界：候选阶段必须跳过该候选，若无替代则留空并产生既有 warning；最终终验若发现已经占用的整轮无休息状态，必须返回 hard error，绝不在终验阶段静默修改 assignment。若整轮累计净消耗为零，按实际 mood budget 允许通过；这不构成宿舍恢复或其他全基建永续证明。

### D6. 自定义班次时长复用同一算法

只要运行时能提供最终 `duration_hours` 数组，D1-D5 不需要增加 profile 分支。验收至少覆盖：

- `12/6/6`：相邻短班可被算法合并为连续工作段；
- `8/8/8`：按实际累计时长重新判断；
- `5/7/10`：不依赖固定班次数或固定下标；
- 跨周期首尾连续工作的情况；
- 多办公室的房间稳定复用。

当前闭集 profile 仍可继续由现有入口提供；本 ADR 不承诺同时开放自由时长 CLI，只要求核心算法不把自由时长排除在模型之外。

## 不在本次范围

- 不实现 Mower 动态换班、实时心情触发或宿舍黑名单；
- 不建立全基建连续心情全局最优模型；
- 不由 export 或 CLI 重新选择办公室成员；
- 不改变生产设施、中枢、Plan anchor、producer dependency 或完整状态互斥；
- 不用办公室连续预算替代既有生产设施 `continuous_work` 或制造心情落差规则；
- 不在 schedule 层新增第二套心情常量或绕过最终房间上下文。

## 实现计划

1. 确认并复用现有 mood canonical/data/code 的 `mood_cap=24`、基础消耗和安全余量；办公室 `0.75/h` 仅作为满中枢上下文下的计算结果，不新增第二套常量。
2. 清理办公室语义中的 `index == 2`、固定 profile 时长和斥罪姓名特判；不得误删深海等真正属于 profile 拓扑的其他逻辑。
3. 提取事务性连续预算 helper：候选临时落位，完成该 shift 最终上下文求值，失败则完整回滚并尝试下一候选。
4. 用 `HashMap<RoomId, Vec<AssignedOperator>>` 保存连续段中的普通办公室成员；绑定成员继续走 resolved dependency、消费方 presence 和原有 bind 路径。
5. 在所有 profile 的最终 report 上运行闭环终验；不缩小现有 `validate_profile_rotation` 的检查范围，并显式处理“整轮无休息边界”：累计净消耗大于零时拒绝，累计为零时允许。
6. 同步更新 `docs/定时换班.md`、`docs/SCHEDULE_ROTATION.md` 所引用的旧办公室范围。ADR 0008 accepted 后，旧裁决必须在正文中明确被本 ADR 替代，不能让 current canonical 保留冲突表述。
7. 通过终态断言与真实 CLI 检查连续预算边界、`12/6/6`、自定义 `8/8/8` 与 `5/7/10`、跨周期首尾、整轮正净消耗拒绝与零净消耗允许、多办公室房间稳定、绑定成员缺席及斥罪数据驱动行为；不新增源码测试。

## 后果

正面后果：

- 规则由实际心情消耗驱动，新增班次时长不需要新增 profile 特判；
- S2/S3 同人只是 `12/6/6` 下的自然结果，不再是 schedule 下标规则；
- 斥罪和未来新增高消耗干员自动获得一致处理；
- 多办公室可以保持成员与房间稳定；
- 候选准入与最终终验共享同一证明逻辑。

代价与限制：

- 候选试填需要重复计算办公室 mood context，可能增加排班求解成本；
- 只有在最终 assignment 已确定后，跨房间心情修正才具有完整语义；
- 该模型证明的是办公室连续工作预算，不证明真实宿舍床位和全周期恢复最优；
- 旧的“办公室普通成员必须属于当班 active cohort”断言需要明确限定为绑定成员和非连续段候选，不能继续覆盖跨 cohort 普通复用。

## 替代方案

### 按 profile 固定 S2/S3

放弃。该方案无法支持自定义时长，且会把班次下标误当作办公室业务语义。

## 与既有裁决的关系

本 ADR 当前为 `proposed`，因此暂不改写 current canonical。若用户接受并进入实现：

- 办公室普通成员跨 cohort 连续复用将替代旧规则中“普通成员仅在活跃 cohort 具备上岗资格”的范围；
- 办公室连续预算将在 ABC 以及其他 timed profile 的最终办公室结果上生效，替代旧规则对 ABC 不做连续工作 hard validation 的办公室例外。ABC 仍不因此获得全设施连续心情、宿舍床位或全基建永续证明；
- 生产设施、中枢、会客室、Mower 和宿舍责任不被本 ADR 改写；
- 其他未被本 ADR 明确替代的排班决策保持有效。

接受本 ADR 后，必须同步更新 `docs/定时换班.md` §1.1/§5.2 和 `docs/SCHEDULE_ROTATION.md` 对 ABC 验证范围的表述，避免“ABC 不做完整循环 hard validation”被理解为办公室也不验证。

### 按姓名特判斥罪

放弃。该方案不能处理未来新增高消耗角色，也会绕过实际房间和中枢上下文。

### 只在最终 report 做警告

放弃。候选选择已经可能产生随后必然失败的办公室安排；该规则需要在候选准入阶段作为 hard constraint，同时在最终结果阶段复验。
