# ADR 0004: 体系审计裁决与排班责任边界

> 文档角色：decision
> 生命周期状态：accepted
> 当前真源：docs/公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md；docs/公孙长乐的体系分析文档/AUTOMATION_GROUP_CHAIN.md；docs/公孙长乐的体系分析文档/RED_PINE_FOREST_CHAIN.md；docs/公孙长乐的体系分析文档/RHINE_LAB_CHAIN.md；docs/FIREWORKS.md；docs/CONTROL_CENTER_ASSIGNMENT.md
> 摘要：保存 2026-07 体系审计中确认的领域语义、排班事实所有权与动态 producer 范围

> 历史决策状态：accepted
> 日期：2026-08-06
> 当前规则：[迷迭香感知链](../公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md)、[自动化组](../公孙长乐的体系分析文档/AUTOMATION_GROUP_CHAIN.md)、[红松林](../公孙长乐的体系分析文档/RED_PINE_FOREST_CHAIN.md)、[莱茵生命](../公孙长乐的体系分析文档/RHINE_LAB_CHAIN.md)、[人间烟火](../FIREWORKS.md)、[控制中枢](../CONTROL_CENTER_ASSIGNMENT.md)
> 实现参考：`data/orchestration_rules.json`、`data/producer_rules.json`、`data/skill_table.json`、`crates/infra-core/src/layout/workforce.rs`、`crates/infra-core/src/cross_facility/collector.rs`、`crates/infra-core/src/response_dependency.rs`、`crates/infra-core/src/layout/orchestrate/plan.rs`、`crates/infra-core/src/layout/orchestrate/select.rs`、`crates/infra-core/src/schedule/team_rotation.rs`
> 历史评审：2026-07 公孙长乐 × InfraCalc 体系审计及用户逐项裁决；早期阶段记录见 [体系审计历史摘记](../ARCHIVE/SYSTEM_AUDIT_HISTORY.md)

本 ADR 保存 2026-07 体系审计中已经接受的决策，不声明这些决策已经实现或验证。当前领域语义以对应 canonical Markdown 为准，当前实现事实以代码、数据和可复现 evidence 为准。

## 背景

体系审计同时发现了两类问题：

- 不同体系文档对 priority、心情阈值、候选顺序和阵营计数存在冲突。
- 部分实现偏离已确认语义，包括莱茵训练室人数遗漏、塑心技能未正确建模、红松负效率归因错误，以及轮换层重新按 tag 推导 producer/consumer 出勤。

这些偏离不能通过增加 schedule 末端校验继续兜底。项目主路径已经规定：

```text
build_plan -> execute_plan -> fill -> resolve -> rotation
```

required admission、体系选型和跨设施依赖必须在对应 owner 中形成 resolved facts；轮换层只消费这些事实并安排班次，不能按干员名、tag、Team、Shift 或 room id 重新裁决上游语义。

本次审计还确认项目定位：

> 本项目旨在完全还原专家排班思路，而不是把项目扩展为通用的跨站 producer 求解器。

因此，本 ADR 既记录具体体系裁决，也限制动态 producer 和轮换层的责任范围。

## 决策

### D1. 迷迭香感知链 priority 统一为 21

迷迭香感知链采用 `priority = 21`，与红松林、莱茵生命文档中的 priority 参照统一。`AUTOMATION_GROUP_CHAIN.md` 中曾出现的迷迭香 priority 18 口径废弃；该裁决不改变自动化组自身的 priority 20。

理由是迷迭香属于应在主要生产体系阶段认领核心、producer 和房间容量的跨设施体系，不能等 registry 落位后再以软候选补入。

影响是 `crates/infra-core/src/layout/orchestrate/select.rs` 中两个编译阶段具有明确语义：

```text
priority >= 19：主要体系段，先于 legacy registry 编译
priority <= 18：late 段，读取主要体系和 registry 落位后的真实空位
```

priority 只决定候选进入编译和比较的时机，不替代 required admission、配方、人数或班次绑定。

### D2. 夕与令的心情边界采用不对称闭区间

夕的状态边界为：

```text
mood >= 12：产出感知
mood < 12：产出人间烟火
```

因此 `mood == 12` 归入感知分支，迷迭香文档中旧有的“严格大于 12 才产感知”口径废弃。

令继续采用：

```text
mood > 12：产出人间烟火
mood <= 12：产出感知
```

理由是这组边界已经由 `docs/FIREWORKS.md` 统一裁决，夕与令不能共享同一个比较符号模板。

影响是机制数据、单班结算和三班心情传播必须在 `11.999... / 12 / 12.000...` 三个边界点保持一致。

### D3. 莱茵生命计数包含训练主位与训练副手

娜斯提和缪尔赛思读取的莱茵生命阵营人数包含训练室主位和训练副手。`docs/公孙长乐的体系分析文档/RHINE_LAB_CHAIN.md` §3 的业务口径胜出；游戏技能文本中的“不包含副手”不作为本项目业务真源。

理由是用户已经明确裁决本项目采用的专家排班计数口径，代码和数据不能反向覆盖 canonical。

影响是 `crates/infra-core/src/layout/workforce.rs` 必须将训练主位与训练副手纳入莱茵计数。该修改只作用于莱茵语义，不得顺带改变杜林等仍明确排除副手的独立计数规则。

### D4. 塑心是感知链收益 producer，但不是感知 producer

PRTS 资料确认塑心「无声共鸣」在精 0 开放：每 1 名同宿舍干员提供 1 点无声共鸣；该资源可由黑键的感知链转化为贸易收益。黑键、塑心和深律均属于该机制来源。

现阶段采用以下实现口径：

- 塑心按固定 5 点 `SilentEcho` 结算。
- 塑心不产出 `Perception`，不参与 50 感知启动门槛。
- 塑心作为感知链收益 producer，随迷迭香核心绑定轮换。
- 塑心最低练度为精 0。
- 深律暂不建模，列为后续开放项。

理由是固定 5 人口径可以表达专家排班中的满宿舍用法，同时避免把全基建宿舍总人数误当作塑心所在宿舍人数。

影响是 `data/skill_table.json`、`crates/infra-core/src/cross_facility/collector.rs` 和 `data/orchestration_rules.json` 必须共同表达精 0、固定 5 点无声共鸣、非感知 gate 和随核心绑定四项语义。

### D5. 自动化组两电两人通用站按声明顺序选人

两物理电站布局中的两人通用制造站，按候选声明顺序选择：

```text
冬时 > 异客 = 掠风
```

不根据当前有效电站数下的理论效率动态重排。`AUTOMATION_GROUP_CHAIN.md` §5.5 的效率表只保留为理论对照，不再作为该分支的选人依据。

理由是该分支保存专家名单顺序 policy，而不是一个匿名的实时效率最大化目标。

影响是规则层必须保留稳定候选声明顺序；制造 solver 仍负责入选组合的实际机制结算，但不能反向改写该分支的业务选人顺序。

### D6. 轮换层不再按 tag 重推动态 producer 出勤

轮换层将删除以下兜底路径：

- `optional_dynamic_control_constraints`
- γ 搜索中的 tag blocking
- 基于 producer/tag/consumer presence 的最终重推校验

动态 producer 的唯一主路径为：

```text
winner 选中
  -> response_dependency.rs 形成 resolved dependency
  -> plan.rs 保存 ExactPresence
  -> shift_binds 消费
  -> producer 与实际贡献 consumer 同上同下
```

γ 班允许普通搜索新增未绑定的 tag consumer。此类 consumer 的收益只按该班最终实际在场状态结算，不要求轮换层为它后置创建 cohort 或反向绑定 producer。

理由是 tag 只能描述候选语义，不能代替 winner、required admission 或 resolved dependency。轮换层重新推导会形成第二个选型 owner，并可能改变 Plan 已经裁决的关系。

该路径明确分类为轮换搜索 `heuristic`：未绑定 producer 可以进入 γ 的普通候选，但最终未必上岗；最终结果按实际在场正确结算，是当前搜索过程得到的 best-found，不承诺给定当班共存上下文下的最优解，更不承诺全基建全局最优。

### D7. 红松赤金 -10% 归属于焰尾

红松林成员制造赤金时的 `-10%` 来自焰尾技能 `control_mp_psk[000]`「红松的骑士」，不属于薇薇安娜。

理由是该归因同时符合 `RED_PINE_FOREST_CHAIN.md` 和游戏机制；当前把负效率 atom 挂在薇薇安娜技能下会造成来源 trace、技能启用条件和独立组合结算错误。

影响是 `data/skill_table.json` 中赤金 `-10%` atom 将从薇薇安娜技能移回焰尾技能。薇薇安娜只保留骑士制造成员 `+7%` 的自身效果。

### D8. 莱茵 priority 12 不新增规则或 registry owner

莱茵二人或三人制造组的 priority 12 只登记在 `RHINE_LAB_CHAIN.md` 中，表示这类候选在主要体系之后进入比较的时机语义。

不为该口径新增 orchestration rule、registry 条目或固定组合。

理由是普通制造 `C(n,3)` 搜索已经是莱茵同房组合的唯一选型 owner；新增 priority 12 规则会重复认领房间和成员，并把动态效率比较退化为固定体系抢位。

影响是后续实现不得以“文档存在 priority 12”为由增加第二条莱茵制造选型路径。

### D9. 娜斯提与缪尔赛思的 0 挂件分支暂不支持

娜斯提或缪尔赛思在没有任何莱茵挂件时单人工作，即 0 挂件分支，暂不进入自动编排。

`data/orchestration_rules.json` 中相关支持角色继续保持 `min_count = 1`。

理由是当前 late competitive 规则以至少一个实际支持位作为启用边界；开放裸奔分支会扩大候选行为，需要单独比较收益、机会成本和轮换依赖。

影响是 0 挂件支持作为 deferred 项保留，不在本次实施中静默放宽。

### D10. 不建立通用跨站 producer 求解入口

动态 producer 范围保持为 `data/producer_rules.json` 登记的四条具名机制：

```text
八幡海铃
戴菲恩
凛御银灰
涤火杰西卡
```

新增使用动态 workforce selector 的跨设施注入技能时，必须同步登记稳定 `source_buff_id`、目标设施、admission 和排班关系。`crates/infra-core/src/response_dependency.rs` 的覆盖校验是硬门禁：技能漏登记或登记错误时，数据加载必须报错。

理由是当前四条机制需要统一的响应依赖协议，但这不等于授权建设通用跨站 producer 产品能力。具名登记和覆盖校验可以防止漏建模，同时保持项目服务专家排班思路的范围。

影响是轮换层只消费 `AssignmentPlan` 中已经 resolved 的 producer facts，不得按干员名、tag、Team、Shift 或 room id 重建 producer/consumer 关系。

## 后果

### 实现影响

本决策接受后的实施分为四个责任单元：

1. **P1：莱茵 workforce 计数**
   - 修改 `crates/infra-core/src/layout/workforce.rs`。
   - 将训练主位与训练副手纳入莱茵计数。
   - 保持杜林等独立排除规则不变。

2. **P2：塑心与自动化候选语义**
   - 修改 `data/skill_table.json`、`crates/infra-core/src/cross_facility/collector.rs` 和 `data/orchestration_rules.json`。
   - 建模塑心精 0、固定 5 点 `SilentEcho`、不进入感知门槛并随核心绑定。
   - 将两电两人通用站恢复为声明顺序 policy。

3. **P3：删除轮换层重推**
   - 修改 `crates/infra-core/src/schedule/team_rotation.rs`。
   - 删除 optional dynamic control constraints、γ tag blocking 和 tag-based final presence 校验。
   - 保留 resolved `ExactPresence`、显式 bind、实际 assignment 结算和通用班次不变量校验。

4. **P4：红松负效率归因**
   - 修改 `data/skill_table.json`。
   - 将赤金 `-10%` atom 从薇薇安娜迁回焰尾 `control_mp_psk[000]`。

这些单元可以分别实施和验证，但不得通过 schedule 特判、固定房号或姓名分派函数替代对应 owner。

### 文档影响

本次裁决需要保持三组文档同步：

1. `docs/公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md`：priority、夕边界、塑心角色、固定 5 点、绑定名单和剩余风险。
2. `docs/公孙长乐的体系分析文档/AUTOMATION_GROUP_CHAIN.md`：迷迭香 priority 参照和两电两人通用站声明顺序。
3. 排班责任边界：`docs/SCHEDULE_ROTATION.md` 与 `docs/ARCHITECTURE_TOUR.md` 必须统一说明 rotation 只消费 resolved facts，不按 tag 重推。

`RHINE_LAB_CHAIN.md`、`RED_PINE_FOREST_CHAIN.md`、`FIREWORKS.md` 和 `CONTROL_CENTER_ASSIGNMENT.md` 继续拥有各自领域规则；本 ADR 不复制或取代其当前实现状态章节。

### 测试影响

后续实施至少需要覆盖：

- 莱茵训练主位、训练副手、去重、娜斯提上限 5 和缪尔赛思排除自身后上限 5；同时证明杜林副手口径未改变。
- 夕和令在 `mood < 12`、`mood == 12`、`mood > 12` 三档的资源产出。
- 塑心精 0 产生固定 5 点 `SilentEcho`，不增加 `Perception`，不使 40 感知错误跨过 50 门槛，并随实际 Rosemary alternative 绑定。
- 两电两人通用站在有效电站变化后仍按冬时、异客、掠风声明顺序选择。
- γ 新增未绑定 tag consumer 时不生成后置 bind，收益按最终实际在场结算；已有 resolved `ExactPresence` 仍严格同上同下。
- 焰尾在场时红松成员赤金 `-10%`，薇薇安娜单独在场时不产生该负效率，作战记录 126% 锚点保持成立。
- `producer_rules.json` 四条登记可加载，新增动态 workforce inject 未登记时覆盖校验失败。
- 真实 `plan` 与 `layout team-rotation` 入口完成回归。

实施前留存的 `plan` 和 `layout team-rotation` 基线记录为 PASS。该记录只用于实施后的差异对照，不证明本 ADR 的裁决已经落地。

### 保留的限制

- 塑心固定 5 点是本阶段接受的实现口径，不代表已经支持按实际同宿舍人数动态选择。
- 深律尚未建模。
- 娜斯提、缪尔赛思 0 挂件分支尚未开放。
- D6 明确允许 best-found heuristic；不得将其输出描述为当班上下文最优或全局最优。
- ADR 只保存决策历史；实现状态变化不回写为 ADR 的“已实现”声明。

## 已放弃的替代方案

### 在轮换层按 tag 重推 producer/consumer

放弃 `optional_dynamic_control_constraints`、γ tag blocking 和最终 tag presence 校验。它们会让 schedule 成为第二个选型 owner，并可能覆盖 winner 已经解析的依赖事实。

### 为莱茵 priority 12 新增规则或 registry 条目

放弃以固定莱茵二人/三人体系抢占制造房。普通制造 `C(n,3)` 搜索继续作为唯一选型 owner，priority 12 只保留候选进入比较的时机语义。

### 用全基建宿舍人数作为塑心 selector

放弃直接复用全基建 `dorm_occupant_count`。该值不能证明塑心所在宿舍的人数，会把其他宿舍成员错误计入；本阶段采用固定 5 点。

### 建立通用动态 producer 泛化入口

放弃从当前四条具名规则扩展出任意跨站 producer 的通用产品能力。新增机制必须先经领域裁决并登记，覆盖校验只保证没有漏登记，不构成扩展范围的授权。
