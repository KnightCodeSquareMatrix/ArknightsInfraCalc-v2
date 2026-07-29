# 动态 Producer A+：设施无关候选列 + 精确索引 Join

> 文档角色：active-change
> 生命周期状态：ready-on-request
> 当前真源：docs/CONTROL_CENTER_ASSIGNMENT.md；docs/ORCHESTRATION_LAYER.md；docs/SCORING_MODEL.md；docs/QUALITY_AND_AUDIT.md
> 摘要：动态 producer 候选列、精确索引 Join 和 winner dependency 任务

> 当前进度：2026-07-19 已完成通用 rule catalog、producer presence 集合比较、完整有序生产
> 前缀、resolved dependency、rotation 消费、runtime embedding 与 safe-reduction 差分证明；
> 当前没有 writer，等待按下述条件恢复 exact joint 或 Bake。
> 当前保证：producer presence 对结构化候选全集精确；多房贸易/制造仍是显式
> `policy_restriction`，不保证 exact joint 或全局最优。
> 恢复条件：产品要求 exact 多房保证、撤销当前 policy restriction、出现错误 winner、新机制
> 无法表达、违反用户确认的性能预算，或用户重新授权。
> 性能口径：当前 debug full-E2 样例约 8.4s 不是验收结论；尚无用户确认的当前性能预算。
> 用户确认方向：凛御银灰、戴菲恩、八幡海铃进入同一套可选中枢 producer 生命周期。
> 算法偏好：枚举会改变机制的有限充分状态，不 Bake 最终阵容；小状态先用 live 精确参考，
> 实测后再选择穷举或有可证明上界的 Join，并决定是否物化条件响应。不得扫描原始全笛卡尔积
> 或使用固定 top-K。
> 本文用途：保留 exact joint 与 Bake 的可恢复 active change；当前实现事实以本文标记的完成
> 状态和 canonical 为准。本文件继续负责 producer 联合候选、winner commit 与 rotation
> dependency 的实施 delta；业务语义与 comparator 分别由对应 canonical 拥有，不再主导通用
> catalog schema 或形成第二评分真源。
> Bake 物化边界、并行生成、分片与运行时查询的当前方案见
> [条件化单房响应 Bake 计划](CONDITIONAL_ROOM_RESPONSE_BAKE_PLAN.md)；本文只记录 canonical
> 业务不变量与 comparator 在联合候选合法性、winner commit 和 rotation dependency 上的实施义务。

> 评分裁决：2026-07-27 用户确认完整候选按具名贸易/制造加权效率比较，当前权重 `1:1`，
> 比值未来允许通过显式 policy 参数调整；布局负责宏观产出结构。
> 设计边界补充：三名动态贸易 producer 仍是首个数值回归面，但本次完成口径扩大为当前
> 已建模跨设施 producer 的统一规则、既有 Plan admission 与 resolved dependency 迁移；
> 同房机制继续复用正式单房 solver。尚无 canonical comparator 的机制必须显式 unsupported
> 或保留已命名 policy，不能借本任务猜测新目标。涤火杰西卡等制造目标不得复制第二套
> pipeline、rotation 或 cache 语义。

## 1. 一句话目标

剩余目标是从同一个 pre-control seed 同时考察合法中枢组合和全部目标房 tuple，把现有有序填房 policy 提升为 exact joint；每个完整临时 assignment 统一 resolve，最后只 commit 一个完整 prefix winner。

已删除“普通前缀 + 每个 producer 分别重跑前缀”、戴菲恩固定 `meta_vina` 和轮换层按名字补救的路径。未来 Bake 只替代候选生成与重复求值；catalog 缺失、过期或不兼容时，必须用同一候选集合做 live 枚举，因此只能变慢，不能换答案。

首期执行器可以只支持 `control -> trade`，但公共模型不得命名或编码成只能服务贸易站的
一次性结构。最终边界是 `joint prefix`：中枢候选改变 support、`used`、全局资源或目标
设施 response 时，由目标域适配器提供候选特征和真实 solver 求值；通用层只负责 logical
mask、signature、合法性 join、winner 一次提交和 resolved dependency。

## 2. 实施前必须统一的真源

业务语义服从用户当前裁决和维护期 Markdown。实施前先读：

- [QUALITY_AND_AUDIT.md](../QUALITY_AND_AUDIT.md)
- [SYSTEM_AUDIT_WORKFLOW.md](../SYSTEM_AUDIT_WORKFLOW.md)
- [CONTROL_CENTER_ASSIGNMENT.md](../CONTROL_CENTER_ASSIGNMENT.md)
- [BASE_ASSIGNMENT.md](../BASE_ASSIGNMENT.md)
- [ORCHESTRATION_LAYER.md](../ORCHESTRATION_LAYER.md)
- [SCHEDULE_ROTATION.md](../SCHEDULE_ROTATION.md)
- [SCORING_MODEL.md](../SCORING_MODEL.md)
- [INTERNAL/SHORTCUT_MATCHING.md](../INTERNAL/SHORTCUT_MATCHING.md)

本轮文档整理已经把用户口径同步到 `AGENTS.md`、`CONTROL_CENTER_ASSIGNMENT.md` 和 `BASE_ASSIGNMENT.md`。实现文档保留 legacy 行为说明，但已显式标记为已知缺口；下一位 Agent 应在删除代码路径的同一提交中清理这些历史段落：

| 位置 | 当前状态 | 实施动作 |
|------|----------|----------|
| `CONTROL_CENTER_ASSIGNMENT.md`、`AGENTS.md` | 已写入三 producer 权威公式与生命周期 | 代码必须服从；实现完成后移除“已知缺口”状态 |
| `ORCHESTRATION_LAYER.md`、`SCHEDULE_ROTATION.md` | `meta_vina` 已标成当前 legacy 行为 | 删除 legacy role 说明，改成统一 joint solver 当前事实 |
| `INTERNAL/SHORTCUT_MATCHING.md` | 已区分 legacy 选型与可保留的最终结算 | 删除选型入口；shortcut 只保留实际组合结算 |
| `data/skill_table.json` | 戴菲恩使用跨贸易站总人数 selector | 改为逐贸易房 room-local 响应规则 |
| `data/base_systems.json` 与生成脚本 | 仍包含 `vina_lungmen` fixed System | 从 registry 和生成源一起删除，不只改生成产物 |

若上述当前 Markdown 还有其他相互冲突的业务表述，必须先逐项列给用户；不得依据现有 top hit 自行折中。

## 3. 用户确认的领域不变量

### 3.1 三名 producer 共用同一生命周期

凛御银灰、戴菲恩、八幡海铃都是**可选中枢 producer**，不是 fixed System、required anchor 或固定套餐：

1. 账号拥有或 producer 在候选池中，不能单独强制进编。
2. 是否入选由同一批 control + trade 联合候选的真实结果决定。
3. 没有实际消费者时动态贸易收益为 0；普通中枢组合可以自然胜出。
4. consumer 可以不上、单走、分居不同贸易站或自然同站；除已有独立 hard core 外，不新增固定三人组。
5. shortcut 只结算已经选中的实际同房组合，不能激活 producer、强塞 consumer 或改变候选优先级。
6. producer、consumer 和其他中枢成员在 winner 产生前都不能写入主 `assignment` / `used`。
7. pipeline 以 atom / deferred rule signature 工作，不按三个人名分别分支。

### 3.2 精确公式、tier 与作用域

设贸易房集合为 `R`，`n_s(r)`、`n_g(r)`、`n_k(r)` 分别是房间 `r` 中实际上岗的叙拉古、格拉斯哥、谢拉格标签人数。

| Producer | 激活 tier | 对每间贸易房 `r` 的注入 | 作用域 |
|----------|-----------|--------------------------|--------|
| 八幡海铃 | E2 / `tier_up` | `5 × Σ(q∈R) n_s(q)` | 跨所有贸易站求叙拉古总人数，然后对各贸易房注入同一数值 |
| 戴菲恩 | E2 / `tier_up` | `10 × n_g(r)` | **逐贸易房本地计数**；`(3,0)` 与 `(2,1)` 不能合并成一个全局总数 |
| 凛御银灰 | E0 起 / `tier_0`、`tier_up` | `10 × Σ(q∈R) I[n_k(q) ≥ 3]` | 逐站判断是否至少 3 名谢拉格，再统计达标贸易站数 |

数值单位是订单获取效率百分点。相同 inject family 继续取最大，不同 family 按现有 `GlobalInjectManifest` 规则相加。

凛御银灰的「商业版图」只包含上述逐站阈值。灵知的「精密计算」、孑 E0/E2 形态和 `karlan_precision` 是另一条独立机制，**不得**由凛御银灰的 producer bit 激活或写进本规则。

### 3.3 实际 consumer 与轮换依赖

只有 winner 中真正贡献数值的成员进入 `ResolvedProducerDependency`：

| Producer | dependency 中的实际 consumer |
|----------|-------------------------------|
| 八幡海铃 | 所有实际进驻贸易站、带 `cc.g.siracusa` 的成员 |
| 戴菲恩 | 各贸易房内实际贡献本房计数的 `cc.g.glasgow` 成员 |
| 凛御银灰 | 每个达到 `n_k(r) ≥ 3` 的贸易房内，构成该阈值的全部 `cc.g.karlan` 成员 |

consumer 集为空时不生成 bind。集合非空时，producer 与这些实际 consumer 同上同下、上 2 休 1；不要求不同贸易站的 consumer 被搬到同房。producer 休息班注入必须为 0，未入选成员不得在 rotation 阶段重新强塞。

### 3.4 候选完整性

- 工具人池是热路径候选边界；动态规则必须按 tag / capability 做结构化扩池，不能因人员不在 standalone 名录而漏掉合法 consumer。
- 完整 catalog 或 live 扩池必须覆盖当前 role、anchor、producer rule 与房间约束声明的全部候选。
- System-only 成员只由明确 plan anchor 引入，不能泄漏到普通池。
- 同一逻辑干员只有一个 logical operator id；E0/E2 variant 不能同时占两个岗位。
- operbox、tier、plan、room capacity、已有 `used` 和禁同房关系都在运行时重新过滤。

### 3.5 含中枢成员的组合分类

不能把所有“中枢成员影响其他设施”的组合都当成首期三个可选贸易 producer，也不能把
公共抽象写死成 `joint_control_trade`。当前已知组合按责任边界分为三类：

#### A. 可选 deferred producer

这类成员是否入选取决于实际 consumer 或目标房 response，单独持有不构成 required
anchor：

| Producer | 目标域 / consumer | 当前处置 |
|----------|-------------------|----------|
| 八幡海铃 | 全贸易站叙拉古 workforce | 首期实现 |
| 戴菲恩 | 各贸易房本地格拉斯哥 workforce | 首期实现 |
| 凛御银灰 | 各贸易房谢拉格三人阈值 | 首期实现 |
| 涤火杰西卡 | 制造站黑钢 workforce | 已接入通用 rule、presence 前缀与 dependency；exact manufacture joint 仍待恢复 |
| 灵知 | 本房谢拉格贸易成员，负效率换订单上限 | 单独裁决 comparator 后接入 trade domain |
| 歌蕾蒂娅 | 基建内深海猎人及其生产技能 | 后续 global/workforce domain；同时受最长工作时间约束 |
| 维什戴尔 | 赫德雷实际进入贸易站时的本房订单上限 | 后续 room-local trade response |

涤火杰西卡与三名贸易 producer 具有相同 bootstrap 问题。当前通用 presence 前缀已能表达并
比较她的实际制造 workforce 贡献，且不复制 pipeline/rotation；完整多制造房 tuple 仍服从
当前有序填房 policy，恢复 exact joint 时必须复用同一 rule 与 dependency。

灵知不能简单折算成动态贸易注入百分点。他的真实 response 同时包含效率下降与订单上限
增加，必须由完整贸易 solver / 单位产出模型求值；不得与凛御银灰共享 producer bit、
signature 或孑 variant。

#### B. System / plan 已确认的硬组合

这类组合先由 `AssignmentPlan` 产出 anchor、producer、constraint 或 degradation，再作为
joint prefix 的 seed/support 输入；joint solver 不负责重新决定其业务激活条件：

- 红松林：焰尾、薇薇安娜与红松制造成员；
- 人间烟火：重岳/令候选组、乌有，纯分支另含桑葚；
- 怪猎：火龙S黑角、麒麟R夜刀与调查团资源链；
- 自动化在相应布局下的森蚺中枢 + Lancet-2 发电支持；
- 迷迭香感知链中由计划和降级规则选出的中枢 producer。

硬组合仍可能改变目标设施 response，所以候选 signature 必须携带其最终资源和 capability；
但不得把 required anchor 降级成“如果效率高就自然入选”。

#### C. 尚待业务审计的 fixed 组合

`data/base_systems.json::lungmen_manu_pair` 当前固定斩业星熊 E2 + 诗怀雅 E0。技能结构
只能证明二者可组合提供全制造 +3% 与全贸易 +7%，不能单独证明它们是缺一不可的原子
System。实施本计划时保持现状，不借机删除或强化；应另行按体系审计工作流确认其原子性。

森蚺、火龙S黑角/麒麟R夜刀等既可能出现在硬体系，也可能携带普通候选可见的效果。
rule registry 必须区分 `plan_required` 与 `deferred_optional` 来源，不能仅按 buff id 或姓名
把同一成员永久归入一种选型类别。

## 4. 当前与剩余问题边界

当前实现已从同一个 seed 枚举 catalog 中全部 producer presence 集合，并让每个中枢候选跑完
system/dorm producer、power、贸易和制造后再比较完整有序生产前缀。贸易和制造 producer 都已
进入 winner 搜索，但每个目标域内部仍按稳定房间顺序填充：

```text
execute seed / required anchors
          ↓
control producer presence candidate
          ↓
system/dorm producers → resolve → power
          ↓
ordered trade fill → ordered manufacture fill → final resolve
          ↓
prefix winner commit once
```

它不宣称全基建或全班次的数学全局最优。power 仍按当前顺序先于生产设施选择，不与生产房做
机会成本联合优化；贸易/制造前房也可能看不到后房最终 workforce。剩余 exact joint 工作只在
受影响闭包内同时物化目标房 tuple，再统一 resolve；不扩为全基建指数搜索。

exact indexed join 可以先以 trade adapter 为第一个 vertical slice，但公共类型继续使用
`DeferredProducerRule`、`TargetDomain`、`JointPrefixCandidate` 等设施无关命名；贸易房特有
计数、role 和 solver response 留在 trade domain adapter。涤火杰西卡已经证明当前 rule、prefix
和 dependency 可复用于 manufacture；未来 exact manufacture joint 也不得复制第二套引擎。

标准 243 的 0 / 1 / 2 间贸易房是首期 Bake 快路径。live 参考实现应把贸易房表示为向量并能正确枚举更多房间；三间以上可以慢，但不能退回按 producer 分别重跑的旧语义。只有 Bake 的响应表可以先限制在已声明 room signature，不兼容布局走同语义 live 枚举。

## 5. 数学模型

### 5.1 有限候选集合

这里的候选全集不是把设施绑定表中的 55 名中枢、77 名贸易干员无条件做全笛卡尔积，而是把项目已经接受的**结构化候选边界**正式化：

- control universe = plan pins / requirements + 所有 deferred producer / 当前 policy 有效技能 + standalone 中枢 + 有界无技能 filler；
- trade universe = 当前 hard/core role 的完整合法伙伴 + standalone plain + 三类 target tag 全员 + shortcut / condition 所需 capability；
- 任何新 atom / tag / role capability 必须先进入结构化扩池；本次查询遇到未知或无法分类的 condition/capability 时，该设施立即扩为完整 live facility pool，再进入同一个 signature-indexed join，不能静默忽略。

工具人表因此真正减少组合量，而 tag / capability expansion 保证本任务相关消费者不会被白名单裁掉。A+ 首期的“精确”是指在这份有文档、有测试的候选 universe 内不使用固定 top-K 或近似剪枝；它仍不宣称对所有设施绑定人员做全基建数学全局最优。

从同一个 execute/pre-control seed 构造：

- `C`：满足中枢目标容量 5（可用合法角色不足时取实际最大人数）、required anchors、candidate requirements、operbox/tier 和现有 `used` 的全部合法中枢组合；
- `P(c)`：中枢候选 `c` 确定后，按当前生命周期放置的 system/dorm producers、resolve 后的 power 结果及其占用 mask；它是候选 prefix 的一部分，不在 winner 前写回主状态；
- `T_r`：贸易房 `r` 满足容量、订单、已有 anchor、禁同房和当前非 producer role 合法性的全部单房组合；
- `U`：进入联合搜索前已经占用的 logical operator bitset。

`meta_vina` 不再定义一个候选层。推进之王、摩根、维娜·维多利亚及其他格拉斯哥成员，只要符合普通贸易候选规则，就与其他组合一起按真实 `final_efficiency` 竞争。

每一行至少携带：

```text
logical_operator_mask
variant/effect signature
room signature
siracusa_count
glasgow_count_in_this_room
karlan_count_in_this_room
role / shortcut capability
stable row id
```

### 5.2 合法联合 tuple

一个联合候选 `x=(c,t_1,...,t_m)` 合法，当且仅当：

```text
c.mask & U == 0
P(c).mask & (U | c.mask) == 0
t_r.mask & (U | c.mask | P(c).mask) == 0       for every r
t_r.mask & t_q.mask == 0                       for every r != q
all anchors / room constraints / role legality hold
```

bitset 只负责姓名互斥和运行时过滤，不负责技能公式。

### 5.3 Effect signature

完整 tuple 先生成小型、可验证的动态签名：

```text
producer_rules(c)
siracusa_by_room = [n_s(r)]
siracusa_total = Σ siracusa_by_room
glasgow_by_room = [n_g(r)]
karlan_qualified_by_room = [I[n_k(r) >= 3]]
karlan_qualified_rooms = Σ karlan_qualified_by_room
external OperatorInBase / OperatorInTrade presence
existing named control/global inject components
room levels / capacities / order kinds
```

join partition 必须保留 `siracusa_by_room`、`glasgow_by_room` 和 `karlan_qualified_by_room`，并验证每个实际 row 的局部值与对应分量完全相等；聚合 total 只用于 response 公式。否则可能在 `siracusa_total=2` 的响应下错误连接出 `2+2`，或在“1 个达标站”下选出两个达标站。Silver signature 不附带灵知 precision 或孑 variant。

### 5.4 求值与比较

求值分为可复用的 control prefix 和 trade tuple 两层：

1. 从同一个 seed 克隆临时 assignment，放入完整中枢组合。
2. 执行该候选对应的 system/dorm producer 与 power 支持阶段，并对这个 control prefix resolve 一次。
3. 对 trade tuple 用 baked row feature 推导完整 effect signature；精确 response 命中时直接读取同一 solver 离线生成的结果。
4. response miss 时，把**所有**贸易房同时放入临时 assignment，调用一次 `resolve_base`，让每间贸易房都看到最终跨房 workforce，再用真实 solver / shortcut 结算；相同完整 signature 可在本轮 memo。
5. 用 `TradeManufactureWeightedEfficiencyV1` 比较完整候选；当前贸易/制造权重为 `1:1`，
   权重必须显式进入 policy 与 Bake 指纹，不得加入公孙人工固定分或未声明分量。
6. 对暂定 winner 必须构造完整临时 assignment 再真实 resolve 一次，并核对 baked breakdown / `rule_id`。若不一致，整次查询拒绝该 catalog 并用 live reference 重跑，不能只修正 winner 分数。
7. 由真实 winner 记录 `ResolvedProducerDependency`，然后一次 commit。

当前评分契约已经由用户裁决为：

1. LMD 核心布局先满足 hard admission。多贸默认优先级为但书 > 可露希尔 > 完整龙巫；
   Rosemary/黑键体系 alternative 激活时，Plan 原子对调后两者，peak 使用但书 + 可露希尔/黑键，
   完整龙巫进入 γ reserve。叙拉古等普通队友不升级为 hard core。
2. 每个完整 prefix 必须同时结算全部贸易和制造房；令 `T=Σ trade.final_efficiency`、
   `M=Σ manufacture.final_efficiency`，使用
   `TradeManufactureWeightedEfficiencyV1 = trade_weight×T + manufacture_weight×M` 比较。
3. 当前默认 `trade_weight:manufacture_weight=1:1`。比值未来可调整，但必须由显式 policy/config
   提供并进入输出解释与 Bake fingerprint；布局负责宏观贸易/制造房间配比。
4. 发电不进入加权总和，只作为约束和 response 支持状态；`ControlInjectRawSumV0` 只保留为
   普通中枢补位的当前 policy，不再决定动态 producer 的最终 prefix winner 或其平局。

该裁决改变目标函数但不改变房间 `final_efficiency`、`DailyTotals` 分域输出或 L1-L3 公式。
当前代码仍使用旧 prefix comparator，因此本段是待实现 contract，不是当前实现快照。

#### 戴菲恩在新 comparator 中的口径

戴菲恩仍按每房 `10 × n_g(r)` 生成真实 response，`(3,0)` 与 `(2,1)` 不能合并逐房
signature。旧的注入百分点和只保留为解释分量；最终 winner 由这些 response 产生的各房
`final_efficiency` 进入 `TradeManufactureWeightedEfficiencyV1`，不再单独比较裸注入和。

同样，灵知的“效率下降、订单上限增加”不能进入裸注入百分点 comparator。灵知是否由
单位产出、`final_efficiency` 或现有固定 role 决定选型，必须另行裁决；首期不得假借
凛御银灰规则顺手改变。

## 6. 首版算法：签名完备的索引 Join

不能直接遍历 `C × T₁ × T₂`。按设施绑定全集计算，仅两个三级贸易站就有数十亿互斥 room pair，再乘中枢会达到不可执行的量级。A+ 的简单之处来自**离线完整行 + 精确响应签名索引**，不是原始笛卡尔积。

### 6.1 只按完全等价签名分桶

1. 生成结构化 universe 内全部 control / trade row，不截固定 top-K。
2. 每个 control row 在当前 seed 下生成 `control + support` prefix row，携带完整 logical mask。
3. control prefix 按“对 comparator 和所有目标房响应完全相同”的 producer/effect signature 分桶。
4. trade row 按 room signature、明确的 `role_rank`、每房局部 faction counts、shortcut / presence capability 分桶；具体静态分数仍留在 row 上，并为每个已支持 control effect signature 烘焙排序视图。
5. 桶内不删除任何 operator mask 不同的行，只按该 control signature 下的真实 room comparator + stable row id 排序。

这种 grouping 是等价类索引，不是 Pareto：bucket key 只折叠动态响应状态，不折叠实际 operator mask。标准双三级贸易站保留局部分区时，三类 producer 的核心计数格上界为 `8 producer masks × 16 Siracusa room vectors × 16 Glasgow room vectors × 4 Karlan-qualified-room vectors = 8,192`，且大量状态不可达。这个数字**不包含** external presence、其他 named inject、room signature、role/shortcut capability 和 tier variant，不能当成完整 bucket 总数或内存上界；Phase 0 必须实测完整维度。账号可用性、具体静态效率和 `used` 冲突仍由桶内每一行保留。

### 6.2 运行时精确 Join

```text
best = None
for control_effect_bucket in compatible_control_buckets:
    for reachable_trade_feature_state in indexed_feature_states(control_signature):
        sorted_views = baked_or_live_row_responses(control_signature, trade_feature_state)
        rows = exact_disjoint_join(
            control_effect_bucket.rows,
            sorted_views.room_rows,
            available_variant_mask,
            used_mask,
            TradeManufactureWeightedEfficiencyV1,
        )
        if rows exist:
            candidate = materialize_exact_row_responses(rows, sorted_views)
            best = TradeManufactureWeightedEfficiencyV1(best, candidate)
validate_best_on_full_live_assignment_or_rerun()
commit(best) exactly once
```

`exact_disjoint_join` 只在完整充分 signature 下工作。每个
`BakedRoomResponse(row_id, exact full signature)` 必须在排序或剪枝前可得；heap / branch-and-bound
必须使用完整加权目标或已证明同单位的可采纳上界，维护 incumbent，并在剩余上界不能胜出时
才停止。现有按 index vector 返回首个无冲突 tuple 的 helper 不满足总和最优性，不能直接接入。
若 late resolve 仍会改变 row score，说明 signature 不充分，必须扩展闭包或穷举全部可行 tuple。

同构贸易房可用稳定 row id 约束消除 `(A,B)` / `(B,A)` 对称重复；房级或订单不同则保留有序 bucket tuple。同一完整 signature 的 live solver 结果可在本轮 memo。cache miss 时现场构建相同 bucket / response，继续走同一个 indexed join，不能退回旧普通前缀或 per-producer pipeline。

首期不预设通用 DP 或 Pareto frontier。先测量充分状态、候选行和可行 tuple：小规模直接穷举，
规模需要时才使用可与暴力 oracle 差分、具有证明上界的 exact branch-and-bound / heap。

## 7. A+ Bake 物化什么

本节保留 joint solver 对 Bake 的接口要求。具体物化模型已经收敛为“单房 row × 中枢效果
签名 × 跨房摘要 → 结构化 solver response”，其 schema、并行生成、分片和 cache gate 以
[条件化单房响应 Bake 计划](CONDITIONAL_ROOM_RESPONSE_BAKE_PLAN.md)为实施真源；若两处细节
冲突，以该专门计划为准，但不得改变本文定义的业务不变量和 comparator。

### 7.1 紧凑行

建议 schema 至少分为：

```text
OperatorDictionary
  logical_id(name)
  variant_id(name, tier, effect signature)

BakedControlRow
  logical mask + variant ids + producer/effect signature
  static named-policy components + ordinary_eligible/source rank + stable row id

BakedTradeRow
  logical mask + variant ids + room signature
  local faction counts + presence bits + role_rank/shortcut capability

BakedRoomResponse
  row id + exact supported effect signature
  true solver efficiency/breakdown/rule_id
```

不物化所有 `LayoutContext` 的笛卡尔积，也不保存一个脱离 signature 的“永久最终分数”。首期可以为三名 producer 的有限组合及常见计数响应多烘焙几份结果；这是小型有限表，不是全局人物 `2^N` mask。

### 7.2 Tier 边界

最简单的首期快速表使用标准 full-E2 variant：

- 八幡海铃与戴菲恩只有 E2 producer rule；
- 凛御银灰 E0/E2 都有相同商业版图规则，full-E2 表使用其 E2 variant；
- 混合 tier、未知 variant 或 catalog 未覆盖的 operator 现场生成对应行并进入同一个 live 枚举；不得把它当成“不存在”。

后续若混合 tier 命中率确有价值，再扩展 `(logical_id, tier_variant)` catalog。logical mask 始终按姓名去重。

### 7.3 工具人池与完整行

工具人池应单独形成连续热区；role anchor、三类 producer 的 target tag、shortcut capability 和 plan anchor 形成结构化扩区。运行时可以先扫描热区，但完整求解必须继续覆盖任务声明的全部扩区。

这正是工具人池的正确作用：减少大多数普通候选的扫描量，同时由 capability-driven expansion 保证稀有组合不会因为不在白名单中消失。

## 8. Cache 兼容与 live 后备

catalog 使用前至少校验：

- schema 与 semantic model version；
- generator fingerprint；
- operator dictionary、producer-rule registry 和 variant model；
- skill table、operator instances、standalone roster、segments、shortcuts、systems 和 baseline layout 的逻辑相对路径 + 内容 hash；
- room signature、tier 假设、mood / shift 假设和 Bake options；
- catalog 自身 bytes/hash、row count 与 index checksum。

统一后备规则：

```text
catalog row/response 精确命中
    → 用 baked row 或 baked solver response
任一 mismatch / missing row / unknown variant / unsupported context
    → live 生成或 live resolve 同一个候选
    → 继续进入同一完整候选行 + 签名索引 Join
```

不能在 mismatch 时改用 top-K、旧 schema 分数或 per-producer pipeline。生成过程写入 generation-id 临时目录，全部文件与 checksum 校验完成后再原子切换；读取方不能只依赖进程内 `RUNTIME_BAKE_IN_PROGRESS`。

## 9. 当前开放边界

| 生命周期 | 当前位置 | 剩余义务 |
|----------|----------|----------|
| exact joint | `layout/assign/pipeline.rs`、贸易/制造 fill | producer presence 已统一比较，但目标房仍按稳定顺序提交；这是显式 `policy_restriction`，前房最优性依赖后房时不保证 canonical exact winner |
| conditional response | `search/joint_trade.rs`、目标域 solver | 只有未接主路径的最小 joint helper；尚无完整 response row、logical mask tuple、统一临时 assignment resolve 与 winner 对账 |
| Bake | `bake.rs` schema v12 单房 baseline 表 | 仍不能表达完整联合 rule/tier/cross-room signature；不兼容时只能安全拒绝，尚无 exact joint 的 live/Bake 等价证明 |
| performance | full-E2 debug rotation | 约 8.4s 只是当前样例，不是已确认预算；在用户确认预算前不据此启动新优化框架或宣称性能达标 |

## 10. 单一责任边界

修复后的唯一责任链：

| 不变量 | 单一负责边界 |
|--------|--------------|
| producer 规则、tier、target 与作用域 | 设施无关 `DeferredProducerRule` / `DeferredProducerSignature` registry，由 atom/capability 提取 |
| 候选全集与 bitset | joint candidate catalog + live candidate factory，共用同一设施无关 row envelope |
| control + 目标设施联合合法性 | `joint_prefix` 枚举器；首期由 trade domain adapter 提供房间行和 comparator |
| 完整跨房公式 | 完整临时 assignment 上的 `resolve_base` + 真实 room solver |
| winner 一次提交 | joint solver 返回的 `JointPrefixCandidate`，包含 control/support/power/trade |
| 同上同下 | winner 生成的 `ResolvedProducerDependency`，rotation 只消费 |
| cache 安全 | Bake manifest/gate；miss 进入同语义 live 枚举 |

如果同一规则仍要在 pipeline、role 和 rotation 分别按名字判断，说明责任边界尚未完成，不得宣称修复。

## 11. 删除与保留状态

已删除或改写：

- Haru-only 白名单和按单个 producer 复制前缀，改为 catalog 驱动的 producer presence 集合。
- `vina_lungmen` fixed System、`meta_vina` 自动 role 及其生成源。
- 按 `control_has_daifeen_e2` / `control_has_haru_e2` 或干员名重推选型、rotation bind 的路径。
- 固定房号、固定班次、当前 top hit 和旧 Vina package 的快照期待。

当前唯一保留路径：

- `gsl_vina_lungmen` 等 shortcut 只结算已选中的实际同房组合，不参与 admission。
- `GlobalInjectManifest` 负责 family 合并，`producer_rules.json` 负责 admission/target/relation，winner dependency 负责排班事实。
- 真实 `resolve_base`、candidate projection、逐房 solver 和通用 rotation 容量校验继续作为当前 policy 路径。
- cache mismatch 继续 live fallback；未来 exact joint 接入时替换有序 fill 的选择边界，不恢复任何已删除的名字特判。

## 12. 实施阶段

当前没有 writer。Phase 1、当前 policy 下的 Phase 4、以及 Phase 5 已完成；Phase 2、3、6
分别承接 exact joint、candidate catalog 和 cache 发布安全，只有命中顶部恢复条件后才继续。

### Phase 0：统一 Markdown 与保存基线

状态（2026-07-27）：用户已裁决 `TradeManufactureWeightedEfficiencyV1` 及当前 `1:1` 默认权重；
实现前仍需保存新的旧 comparator 基线，并证明依赖闭包与候选行完整。不能复用 2026-07-19
的 8.4s 样例作为预算或性能验收。

### Phase 1：正确的 rule 与逐房 scope

状态（2026-07-19）：已完成 `producer_rules.json`、稳定 source buff capability、四条当前动态
workforce rule 的覆盖校验，以及 Haru 全贸易、Daifeen 当前房、Silver 达标房、Jessica 全制造
workforce 的 live 结算和单元测试。

预计涉及：

- `types.rs` / `global_resource/inject.rs`
- `control/interpreter.rs`
- `layout/workforce.rs` / `layout/resolve.rs`
- `data/skill_table.json` / `operator_instances.json`

交付：通用 deferred rule；Haru 跨站总数、Daifeen room-local、Silver 每站阈值的独立单元测试。

### Phase 2：先写 live reference indexed join

状态：`ready-on-request`。这是剩余 correctness/conformance 主项；当前有序填房 policy 不冒充
本阶段已完成。

建议新建独立模块：

```text
search/deferred_producer/
  rule.rs
  signature.rs
  dependency.rs
search/joint_prefix/
  candidate.rs
  facility_domain.rs
  trade.rs
  join.rs
  comparator.rs
```

交付：候选 row、logical/variant mask、每个 control 候选的 system/dorm/power 支持 prefix、完全等价 signature bucket、best-first disjoint join、完整临时 assignment resolve、一次 winner commit。此时即使没有 Bake，也必须语义正确。

`facility_domain` 定义目标设施适配边界；首期只实现 trade adapter。公共 rule 至少能表达
`TradeAllRooms`、`TradeCurrentRoom`、`ManufactureAllRooms`、
`ManufactureCurrentRoom`、`PowerSupport`、`GlobalResource` 这些目标类别，但未启用的类别
必须显式返回 unsupported 并走同语义 live 路由，不能静默当收益为 0。

### Phase 3：A+ candidate catalog

预计涉及 `bake.rs`，规模足够大时再拆 `bake/candidate_catalog.rs`：

- 紧凑 control/trade row；
- E2 快路径 response；
- effect-signature index；
- bitset runtime filter；
- live miss 接口与差分校验。

### Phase 4：Pipeline 集成与旧路径删除

状态（2026-07-19）：已完成 producer presence 子集枚举、完整
`control → producer/dorm → power → trade → manufacture → resolve` 前缀比较，并删除
`vina_lungmen` fixed System、`meta_vina` role 和按名字补 producer 的 rotation 路径；完整 response
row / logical mask joint boundary 仍依赖 Phase 2。

当前 live 路径使用两项 safe reduction：`must_include` 在组合生成前锁定；正收益规则按目标设施
满席上界排序且只在“当前真实 winner 严格大于上界”时剪枝。读取未完成 support presence 的
control inject 自动返回无限上界、禁用剪枝。full-E2 定向证据中上界准备由约 16.4s 降到约
14ms，完整轮换约 8.4s；这不是 A+ response catalog 的替代品。

预计涉及：

- `layout/assign/pipeline.rs`
- `layout/assign/control_fill.rs`
- `layout/assign/trade_fill.rs`
- `search/role_pick.rs`
- `data/base_systems.json` 与生成脚本

交付：单一 joint selection boundary，并完成第 11 节删除清单。

### Phase 5：通用 rotation dependency

状态（2026-07-19）：`AssignmentPlan.resolved_producer_dependencies` 已统一承载 hard Plan relation
和动态 winner dependency，`shift_binds` / team rotation 只消费规范化结果；解释/导出和更多关系
类型的端到端证明仍待回归矩阵验收。

预计涉及：

- `layout/orchestrate/plan.rs`
- `schedule/team_rotation.rs`
- `schedule/shift_bind.rs`

交付：三种 rule 共用 `ResolvedProducerDependency`；实际 consumer bind、休息班关闭、中枢满 5。MAA exporter 只消费 core assignment，不新增机制。

### Phase 6：Cache 生成安全与验证

- generation-id 临时目录 + 原子切换；
- catalog 自身 hash/bytes/row count/checksum；
- `infra-cli bake validate` 输出精确 mismatch 原因；
- 默认 243、混合 tier、非标准 layout、cache hit/miss 的差分验证。

## 13. 回归矩阵

### 13.1 Rule / scope

- Haru E0 不激活；E2 对 0 / 1 / 2 / 多个实际 Siracusa trade consumer 分别为 `0 / 5 / 10 / ...`。
- Daifeen 仅 E2 激活；`(3,0)` 与 `(2,1)` 逐房结果不同，制造、宿舍和仅在基建内不计数。
- Silver E0/E2 均激活；`3+0` 命中 1 站，`2+1` 命中 0 站，两站各 3 人命中 2 站。
- Silver 不激活灵知 precision，也不改变孑 variant。
- 同 family 最大、跨 family 相加。

### 13.2 联合索引 Join

- 普通路径不选消费者，但 producer + consumer 联合后胜出的 bootstrap。
- 有消费者但普通 control 仍胜出。
- 三 producer 分别胜出、两两共存、三者共存和全部落败。
- control、贸易房 1、贸易房 2 的 logical mask 无交集；冲突时能扫描到更深的合法行。
- 同构房对称去重不改变 winner；不同房级保持有序。
- 工具人热区之外、由 tag/capability 扩入的合法 consumer 可以胜出。
- winner 前主 assignment/used 不变，winner 后只提交一次。
- 完整 tuple resolve 让第一间贸易房也看到第二间房最终 workforce。

### 13.3 Role / shortcut

- 删除 Vina fixed/meta role 后，格拉斯哥组合可因实际效率自然胜出，也可自然落败。
- 多贸默认但书 > 可露希尔 > 完整龙巫；Rosemary alternative 激活时进一步固定但书 + 可露希尔/黑键，
  龙巫进入 γ reserve；同一核心布局内的普通队友按真实效率联合竞争。
- Shortcut 只结算实际同房组合，不决定 producer 激活与进编。
- `gsl_vina_lungmen` 若保留，只在实际成员和 resolved producer gate 同时满足时命中。

### 13.4 Rotation

- dependency 只含实际贡献 consumer；0 consumer 不绑定。
- 三 producer 与各自 consumer 的 presence vector 一致，跨站不强制同房。
- producer 休息班 dynamic effect 为 0。
- 每班中枢 5 人、生产房满编、同一班无重复干员。
- 未入选成员不会被 gamma 或 unavailable 特判重新强塞。

### 13.5 Bake / fallback

- schema、generator、输入、layout、tier、rule registry、catalog hash 任一变化都拒绝旧 response。
- 未烘焙 tier / operator / room signature 现场生成并进入同一 signature bucket / indexed join。
- Bake hit 与强制 live 的候选集合、winner、breakdown 和 `rule_id` 完全一致。
- 损坏/反序列化失败安全回到 live，不传播半张 catalog。
- 生成中断不会让读取方看到新旧文件混合。

### 13.6 共享边界与后续域

- 红松林、人间烟火、怪猎、自动化和迷迭香的 required anchor / degradation 不被
  deferred solver 降级为软候选。
- `plan_required` 与 `deferred_optional` 可同时出现在同一 control row，且 logical mask
  仍保证同一人只占一位。
- 涤火杰西卡已接入 winner 搜索；未来用其规则构造 schema/serialization 与 exact-joint 差分，
  证明 manufacture target 无需新增另一套 rule、dependency、manifest 或搜索引擎。
- 灵知与凛御银灰 signature 相互独立，凛御银灰不会激活精密计算或孑 variant。
- unsupported target domain 明确触发 live/unsupported 结果，不会被错误当作零收益 baked hit。

## 14. 性能验收

不在计划中硬编码无来源的秒数或“必须下降 35%”。Phase 0 先在同一机器、构建模式、fixture 和预热条件下保存基线，再由用户确认目标。

截至 2026-07-19，用户尚未确认当前性能预算。full-E2 debug rotation 约 8.4s 只描述一个样例，
既不证明性能达标，也不单独构成恢复 Bake 的授权；恢复条件是违反届时用户确认的产品预算。

至少记录：

- `|C|`、每个 `|T_r|`、effect-signature bucket 数；
- logical mask / operbox / used 拒绝数；
- 兼容 tuple 检查数与对称去重数；
- baked row/response hit、live row generation、live resolve 和 memo hit 数；
- catalog 行数、磁盘大小、加载时间和内存；
- peak joint 阶段、完整 `plan` 与 rotation 总耗时。

验收顺序：

1. live reference 与 Bake winner 完全一致；
2. 不再随 producer 数量重复跑完整 prefix；
3. cache miss 只增加耗时；
4. 若穷举或签名索引 join 已满足实际使用，不引入更复杂的 DP/Pareto/B&B；
5. 只有实测证明仍慢，才单独提交可与 live reference 差分验证的优化提案。

## 15. 后续 producer 如何扩展

新增 producer 按 effect signature 和 target facility 扩展，不向 pipeline 加名字分支，也不建立一个覆盖所有人物的全局 `2^N` mask：

- **涤火杰西卡**：作为首个 manufacture domain 扩展，使用现有设施无关 rule envelope 与
  `JointPrefixCandidate`；manufacture adapter 提供 Blacksteel workforce 行和逐房真实
  response。不得新建平行 pipeline/catalog，也不把自然入选的黑钢或标准化成员升级成
  hard bind。
- **灵知**：作为 room-local trade response 扩展；先裁决负效率与订单上限的 comparator，
  再由真实贸易 solver 求值。与凛御银灰商业版图、孑 variant 分开建模。
- **歌蕾蒂娅**：作为 global/workforce rule 扩展；除生产 response 外还必须携带最长工作
  时间约束，不能仅凭效率 join 生成 12 小时主班。
- **维什戴尔**：作为特定贸易 consumer 的 room-local limit response 扩展；只有赫德雷
  实际在岗且 producer 实际入选时形成 dependency/capability。
- **火龙S黑角**：后续作为 control → global resource / Monster Hunter consumer rule，显式描述木天蓼、贸易/制造 target 与资源 gate。

这些扩展只用于验证抽象可扩展性，不属于首期三 producer 的功能验收。实施前必须分别完成
领域不变量、作用域、tier、实际 consumer、comparator 和轮换审计。怪猎、人间烟火、
红松林等已确认硬体系继续由 plan 激活；设施无关 rule 只描述其响应，不接管其硬核心选型。

## 16. 非目标

- 宿管恢复、完整心情预算和全基建连续时间最优化。
- 首期把 control、trade、manufacture、power、office 一次合成全局指数搜索。
- 为当前全精二 lineup、固定 room id 或班次下标写策略。
- 用 catalog 结果替代最终完整 assignment resolve。
- 用公孙人工固定效率给 producer 或 package 排名。
- 在没有实测必要性时提前建设通用 DP、Pareto 或 branch-and-bound 框架。

## 17. 完成证明

最终交付逐项填写：

| 不变量 | 唯一代码保证 | 删除的冲突 | 回归 | 真实 CLI | Bake/live 证据 |
|--------|--------------|------------|------|----------|----------------|
| 三 producer 自然可选 | rule registry + joint enumerator | Haru 多前缀 / Vina fixed role | 0/1/multi/bootstrap | 243 + 最小 operbox | winner 完全一致 |
| Daifeen 逐房 | room-local signature + resolver | trade-sum selector | `(3,0)` / `(2,1)` | 两贸易房明细 | response/live 一致 |
| Silver 逐站阈值 | `tag@min` room vector | aggregate / precision 混用 | `3+0` / `2+1` | 跨站 layout | signature hash |
| winner 一次 commit | `JointPrefixCandidate` | 分支提前写 used | seed 不变 / duplicate | profile + MAA | miss 只变慢 |
| 通用轮换 | resolved dependency | producer 特殊调度 | bind/rest/control5 | 三班输出 | 不适用 |

最终回复必须另列：

- 根因层和旧模型为什么允许非法状态；
- 新的单一事实源；
- 删除的 data/code/test 路径；
- build、定向测试、full suite、真实 `plan`、Bake hit/miss 与性能日志；
- 本轮新增失败、既有失败和未验证风险；
- profile / MAA JSON 的可点击路径与 commit hash。

缺少任一项，不得宣称 A+ 已完成。
