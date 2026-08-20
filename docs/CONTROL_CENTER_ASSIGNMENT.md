# 控制中枢排班规则

> 文档角色：canonical
> 生命周期状态：current
> 领域键：facility.control-assignment
> 当前真源：self
> 摘要：裁决中枢候选、注入和全局资源编制

> 本文为控制中枢体系审计的仓库内真源。完整规则以用户提供的
> `CONTROL_CENTER_ASSIGNMENT.md` 为基础；用户裁决内容优先于旧实现。

## 已确认硬规则

控制中枢目标容量为 5 人。中枢入驻角色即使没有中枢技能，也会因中枢满员产生的全局心情减免而具有有效收益；因此编排必须优先填满 5 个合法中枢席位。

补位顺序为：

```text
体系必需中枢位 > 有效产能/解锁入口 > 体系外效率散件 > 心情减免 > 无技能普通角色/其他补位
```

只有在可用且合法的中枢角色少于 5 人时，才允许实际输出少于 5 人；不得因角色没有中枢技能而提前停止补位。

## 自动选择口径

公孙长乐提供的 `25 / 20 / 15`、`15`、`9`、`8`、`40` 等体系赋值只作为人工分析与优先级解释参考，不作为自动排班的固定评分，也不用于硬编码候选排列。

自动排班应优先使用 solver 已经结算的实际技能效率自然搜索。全贸易、全制造等已建模收益不得再叠加一份体系赋值，以免重复计分。数值相同的候选只需要稳定 tie-break，不应通过匿名权重改变实际效率排序。

同一具名 policy 完全同分时，替代关系只消费中枢结算 trace 已证明的
`source_buff_id + inject_family` 事实：实际为 `Winner` 或 `TiedWinner` 的声明替代源才可获得稳定偏好；被同族更高值覆盖的 `Suppressed` 源不视为有效代表，等价候选优先选择没有该冗余源的组合。该 tie-break 不改变注入数值、family max 或跨设施目标函数。

只有以下无法由普通单房效率搜索保证的领域不变量可以成为硬约束：

- 中枢在合法可用角色足够时必须填满 5 人；
- 已激活体系的 required anchor 不得被后续补位替换；
- 依赖贸易或制造消费方的 producer 必须按本班实际落位判断是否有效；
- 规定为完整组合的体系必须原子启用或关闭；
- 明确要求同上同下的跨设施成员必须生成轮换绑定；
- 明确的最长工作时间必须由排班层保证。

某项收益尚未被 solver 建模时，不得直接使用上述人工赋值填补。应先确认该资源是否属于自动排班目标，再以具名、可解释的 policy 分量建模；否则只保留为文档参考。

## 已确认跨设施与组合规则

只有用户明确确认为“同上同下”的跨设施体系，才将中枢 producer 与消费方写入 hard `shift_bind`。该关系由统一计划的轮换绑定保证，不能只检查账号持有，也不能依赖当前队伍切分结果碰巧同班；普通效率耦合不得据此升级为固定成员或班次。

### 通用 producer 责任边界

跨设施 producer 共用一套规则与结果协议，但不共用一种 admission：硬体系继续由 Plan 保证
required 成员，可选 producer 由实际受影响设施的完整候选结果竞争，普通同房或全局观察量不
因此升级为固定组合。技能公式、tier、selector、action 和 phase 只由 EffectAtom 与目标域
solver 负责；编排规则只声明 admission 类别、需要联合的设施范围和排班关系，不复制公式。

同房机制仍由单房 solver 一次选完。跨房、跨设施和全局资源机制只在互相改变分数或人员占用
的影响闭包内联合比较，并使用调用方显式命名、可分解的 policy；producer 层不得自行匿名
相加不同设施分量。live 求值是正确性基线，Bake/cache miss 只能变慢，不能换候选或 winner。

winner 只输出最终结算中仍然有效的 producer 贡献及其最小充分 consumer 集合；同 family
覆盖、上限截断或 shortcut 替代后无效的贡献不生成依赖。排班层只消费细粒度的同上同下、
单向在岗和最长工时事实，再统一求闭包；不得按干员名、tag、Team、Shift 或 room id 重推。
硬体系可显式要求绑定全部成员，可选门槛存在多个等价 witness 时使用稳定的最小集合。

自动求解遇到未分类的跨设施机制或缺失的跨设施 comparator 时必须明确返回 unsupported，
不能默认普通候选、按 0 结算或静默不绑定。游戏技能自身点名干员时，逻辑干员 id 只能作为
规则数据参数，运行时代码仍使用通用 presence/capability 条件。

Mujica 不设“未满五人即非法”的原子启用门禁。其完整五人组合通常会因实际效率更高而由搜索自然选出；成员不足时仍按各自已建模的实际收益参与搜索，不使用人工固定赋值强制整队。

### 动态贸易 producer

戴菲恩、八幡海铃服从可选动态 producer 生命周期：中枢组合与全部实际贸易房候选联合求值后，才决定是否入选。凛御银灰的 producer rule 保留用于识别银灰 consumer 阈值，但属于自动中枢排除项，不进入 producer presence、普通 filler 或轮换 fallback。无实际消费者时不得生成凛御银灰动态依赖。

设 `n_s(r)`、`n_g(r)`、`n_k(r)` 为贸易房 `r` 中实际上岗的叙拉古、格拉斯哥、谢拉格人数：

| Producer | 生效 tier | 对每间贸易房 `r` 的订单效率注入 |
|----------|-----------|----------------------------------|
| 八幡海铃 | E2 | `5 × Σq n_s(q)`；跨所有贸易站统计叙拉古总人数 |
| 戴菲恩 | E2 | `10 × n_g(r)`；**只统计当前贸易房**，不同房间不得先求总和 |
| 凛御银灰 | E0 起 | 仅作为保留的 consumer 阈值事实；自动中枢 admission 为 excluded |

凛御银灰的「商业版图」与灵知的「精密计算」/ 孑形态完全独立，不得共享 producer bit、pool variant 或轮换特判。格拉斯哥组合可因实际 `final_efficiency` 自然胜出或落败；戴菲恩不能激活固定 `meta_vina` 优先站。

winner 只为实际贡献者派生同上同下关系：八幡海铃绑定实际叙拉古贸易成员；戴菲恩绑定各房实际格拉斯哥成员；凛御银灰只绑定达到三人阈值房内的谢拉格成员。不同贸易站不因此被强制搬到同房。完整实施交接见 [DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

用户于 2026-07-27 裁决：动态 producer 的完整候选在 resolve 后使用
`TradeManufactureWeightedEfficiencyV1` 比较，当前贸易与制造权重为 `1:1`。权重属于未来
可调整的具名产品 policy；布局负责宏观贸易/制造房间配比，发电不进入该加权总和。具体公式
和实现状态由 [SCORING_MODEL.md](SCORING_MODEL.md#21-动态-producer-联合候选的具名加权-policy) 统一拥有。

## 当前实现状态与已知缺口

- 中枢搜索在合法候选足够时只生成 5 人组合；无技能普通角色可作为有界 fallback 补位。
- `data/producer_rules.json` 以稳定 `source_buff_id` 登记 admission、目标设施与排班关系，并与其他 solver runtime 数据一起嵌入独立二进制；加载时会与 `skill_table` 的全部动态 workforce 注入做覆盖校验，漏登记或错登记直接报错。
- Peak 把拥有实际 consumer tag 的 deferred producer 作为一个 presence set 做幂集枚举；每个子集从相同 seed 求精确中枢组合，候选重跑 `producer/dorm → power → trade → manufacture → resolve`。公开默认由具名 `ControlInjectRawSumV0` 比较；shadow 候选会刷新每个完整候选的贸易/制造快照后改用 `TradeManufactureWeightedEfficiencyV1`。人数不足只淘汰当前子集，真实 resolve 错误继续向上传播。
- `must_include` producer 在组合生成前锁定，只枚举剩余中枢席位。每个 presence set 先以目标设施满席命中的正收益上界排序；只有上界严格低于当前真实 winner 才剪枝，平局不剪。若 active control buff 读取尚未完成的发电/贸易/基建 presence，上界为无穷并自动退回完整 live 前缀比较。
- 八幡海铃、戴菲恩使用同一贸易响应规则路径；凛御银灰规则只保留 consumer 阈值和数据覆盖识别，不参与自动 producer admission；涤火杰西卡使用同一制造响应规则路径。
- winner 的 `resolved_producer_dependencies` 记录 rule、source buff、实际 producer、实际 consumer、目标房、关系和有效贡献；hard Plan relation 也先规范化为同一结构，`shift_binds` 和 αβγ 只消费这些 resolved facts。
- `vina_lungmen` fixed System、`meta_vina` role、按名字推断八幡/戴菲恩/灵知的 rotation 路径已删除；`gsl_vina_lungmen` 仅保留实际同房组合结算。
- 涤火杰西卡关系为 `none`：每班按实际黑钢 workforce 重算，不与水月、香草、杰西卡或标准化组合固定 team / `shift_bind`。
- 当前 producer presence 集合枚举对结构化候选全集是精确的。`must_include` 锁定和正收益上界属于 `safe_reduction`；测试会从同一 seed 分别启用、关闭剪枝，并核对最终 assignment、`ControlInjectRawSumV0` key 和 resolved dependency，平局不得剪枝，未完成 support 输入返回无穷上界。
- `TradeManufactureWeightedEfficiencyV1` 已接入候选 prefix winner，但尚未晋升为公开默认；该
  policy 自动禁用旧 raw-inject 剪枝，只保留其上界排序作为 `search_heuristic`。后续若要恢复
  剪枝，必须证明新的同单位安全上界；exact joint 仍未实现。
- 贸易/制造多房内部仍使用既有有序填房，明确分类为 `policy_restriction`。当前结果只能在该具名 policy 限制空间内解释，不保证 canonical 完整空间的 exact joint 或全局最优；最终 refresh 不能补回前房依赖后房时已经错失的候选。
- 完整条件响应行和 exact joint 仍是 correctness/conformance 开放项，Bake 是与其分开的性能工作。产品要求 exact 多房保证、撤销当前 policy restriction、出现错误 winner、新机制无法表达或用户重新授权时，应恢复 A+ TODO；不能把它降级成只有复现 bug 后才处理的性能优化。
- 纯人间烟火不使用有序 `pick_one`：计划只声明重岳/令候选组最低命中数，
  中枢 solver 在满足约束的五人组合中按实际效率自然选择。
- 感知启用时纯分支互斥；全图鉴烟火附带分支要求重岳、令同时实际进编。
- 人间烟火的中枢成员在 peak 实际入选后由动态 bind 归入乌有 cohort，活跃班先 pin
  再补满五人；未入选的候选不会在轮换层被重新强塞。
