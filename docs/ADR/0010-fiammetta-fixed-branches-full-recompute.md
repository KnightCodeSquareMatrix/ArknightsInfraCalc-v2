# ADR 0010: 菲亚梅塔固定策略分支与完整排班重算接入

> 文档角色：decision
> 生命周期状态：proposed
> 当前真源：docs/菲亚梅塔使用规则.md；docs/定时换班.md；docs/SCHEDULE_ROTATION.md
> 摘要：菲亚梅塔作为阶段间固定策略分支选择器，N/A/B 三候选各自从同一 Box 完整重算整个排班；候选不预排锁房，靠 L1 shortcut 自然选中 + trade_core 偏好改写上班优先级，菲亚对固定两人各换一次心情，按完整周期收益选 winner

> 日期：2026-08-30
> 决策状态：proposed
> 实现状态：未实现，本 ADR 同时是分阶段实现与逐阶段复审的冻结计划
> 评审状态：等待用户逐 stage 复审

## 背景

### 已确认的原始设计

菲亚梅塔（Fiammetta）在定时换班中的职责是"阶段间的固定策略分支选择器"：

```text
让目标干员获得额外心情 / 工作资格
→ 进入下一阶段的可用候选
→ 下一阶段重新求解完整排班
```

完整流程是：

```text
阶段输入 → 求解本阶段完整 assignment → 菲亚梅塔交接
→ 更新下一阶段输入 → 重新求解下一阶段完整 assignment
```

菲亚梅塔不是：

```text
指定目标原房间 → 清空原房间 → 局部补两个队友
```

### 旧实现为什么产生错误

此前设计明确拒绝回溯重算完整排班，因此实际采用了低成本路径：

```text
已有排班 → 选择菲亚目标 → 只重填目标所在房间 → 尽量不扰动其它 assignment
```

当前代码路径为：

```text
apply_profile_fiammetta_rotation
→ refill_fiammetta_target_room
→ refill_trade_room_with_core / refill_manu_room_with_core
```

这不是偶然实现错误，而是旧约束"禁止回溯重算"的直接结果。服务器真实反馈暴露了该方案的局限：

```text
菲亚前：trade_2 = 卡夫卡 + 巫恋 + 龙舌兰
菲亚后：trade_2 = 涤火杰西卡 + 卡夫卡 + 可露希尔
```

已用服务器真实 ELF 和相同输入确认：γ 阶段组合正确、菲亚处理前的 S2 正确，错误组合在
`apply_profile_fiammetta_rotation` 后产生。

### 本次会话中已撤销的错误实现

本次会话曾提交 `126c2a9`（"普通ABC引入菲亚候选选择层并预冻结完整巫恋站"）与
`050ec79`（"菲亚候选层改为预排中间态构造并真实钉岗"）。经用户复审与实测确认，
预排中间态方向存在结构性缺陷：

1. **A 候选"假钉岗"**：A 只跑普通排班后找到但书本来就在的班次写动作标记，但书
   本来就在岗（`trade_core` 已把但书选为持续核心），收益与 N 完全相同。
2. **预排 seed 锁房与 γ 搜索抢房**：A 分支 seed 把但书钉在首个金单站，但 γ（替补）
   搜索看到的共存 assignment 中该站为空（seed 只影响 peak 不传播到 γ），把巫恋组
   填进同房，随后但书 refill 覆盖它，巫恋组被浪费，trade_2 只能选非预期组合。
3. **B 落位顺序曾被纠正**：B 需要先在 Plan admission 前冻结完整巫恋站，而不是让
   巫恋按搜索规则进驻后被其它 trade core 挤掉。
4. **前置资格判断缺失**：分支选择前没有检查用户是否拥有角色并达到练度。
5. **手搓规则**：曾用"巫恋精二 + β裁缝 + 三级站"手写条件，替代既有龙巫组合判定。
6. **8844 路线收口**：`fiammetta_8_8_4_4` 路线未良好实现，按用户裁决不接入候选流程。

**结构性结论（2026-08-30 实测）**：`schedule_abc_rotation` 是约 1.3 万行的单体
编排（peak → resolve → 建池 → 切半 → γ 替补 → 中枢三班 → 组装评分 → 菲亚后处理 →
终验），候选分支的"预排钉岗"需要同时作用在 seed、γ 搜索、菲亚后处理三个互不感知
的阶段，任何单点修复都会在另一阶段产生连锁回归。因此放弃预排锁房路径，改为本
ADR 的新方向（见 D7）。

### 新方向的实测依据

用 33 个真实 box 的 N 分支（无预排、无锁房）实测：

- 08-212（但书精2、可露希尔精2、巫恋精2+龙舌兰精0）：但书站、可露希尔站自然被
  选中，巫恋+龙舌兰+折光在多个班次自然选中（命中 `gsl_witch_long0_beta`）；
- 44-353（但书精0、可露希尔精0、巫恋精0）：但书站自然在岗，巫恋组因练度不足
  不可用（正确排除）。

结论：角色达标时，但书 / 可露希尔 / 巫恋组已经自然被 trade 搜索选中，无需预排
锁房。搜索按 `trade_efficiency_sort_key` 总效率降序取 top-K（`search/trade.rs:359`），
L1 shortcut 组合效率高自然优先。

## 决策

### D1. 菲亚梅塔是固定策略分支选择器，不是局部 refill

取消"禁止回溯重算"限制。菲亚梅塔按以下固定流程接入：

```text
同一 Box
  → 前置资格判断（用户拥有角色 + 达到练度）
  → 固定枚举 N / A / B 三候选
      N：不使用菲亚梅塔的完整 pipeline 基线（兜底，参与比较）
      A：但书 / 可露希尔单核心（不预排锁房），trade_core 偏好改写上班优先级
         + 完整 pipeline，菲亚对但书、可露希尔各换一次心情
      B：完整巫恋站（不预排锁房），靠 L1 shortcut 自然选中 + 完整 pipeline，
         菲亚对巫恋、龙舌兰各换一次心情
  → 每候选独立完整走一遍现有 pipeline，状态隔离
  → 比较完整周期收益（沿用当前 profile 既有目标）→ winner
  → 8844 不收编
```

这不是通用动态回溯框架，而是少量、固定、明确的策略分支各自完整求解。

### D2. 每个候选从同一原始 Box 独立完整求解

- N/A/B 都必须从同一初始 Box 状态开始，各自走完现有完整排班 pipeline；
- 分支之间不能复用可变 assignment：`assignment`、`used`、`teams`、`H1/H2`、
  中枢结果、宿舍结果、阶段轮换状态；
- 可以共享只读或静态数据：`layout`、`operbox`、skill table、operator instances、静态 pool；
- A 分支不能污染 B 分支，B 分支不能污染 A 分支；
- 每个候选独立生成自己的完整排班和切半结果，不能先用一个分支的切半结果再去给另一个分支。

### D3. 候选分支只改写上班优先级，不固定房间、槽位或干员位置

- 分支不决定"哪名干员必须钉在哪个房间"；
- A 分支通过 `system_preferences`（`trade_core=<alternative_id>`）偏好 trade_core
  选择但书（或按降级规则选可露希尔）作为核心，其余房间、槽位、队友由完整排班
  搜索决定；
- B 分支不干预巫恋组的落位，靠 L1 shortcut（`match_witch_group_shortcut`）自然命中；
- 不得针对单一 operbox、room id、Shift 下标、fixture 或当前 top hit 写补偿特判。

### D4. 分支定义与降级规则

- **N**：不使用菲亚梅塔，普通完整 pipeline。
- **A**：但书 + 可露希尔单核心策略。按布局降级为一人：
  - 单贸易站 153 布局 → 可露希尔；
  - 其余布局 → 但书；
  - 以候选干员实际拥有情况优先。
  - 菲亚动作对 A 的目标各换一次心情：但书一次、可露希尔一次（若可露希尔精二）。
- **B**：巫恋 + 龙舌兰 + 合法裁缝完整站（原子组合，靠 L1 shortcut 自然选中）。
  - 完整巫恋站条件不足（缺角色、未达练度、无合法裁缝、无三级金单站）时，
    **直接放弃 B 分支**，不允许降级为单人；
  - B 的合法组合判定完全复用既有 `match_witch_group_shortcut`（裁缝 β / α / 白板档位
    与适用练度由该项目统一裁决），不得手搓第二套条件；
  - 菲亚动作对巫恋、龙舌兰各换一次心情。

### D5. 前置资格判断

进入分支比较之前，先判断"是否有开启分支的必要性"，依据是用户是否拥有角色且达到练度：

- A：存在金单贸易站，且但书任意练度或可露希尔精二至少其一；
- B：存在三级金单贸易站，且 `match_witch_group_shortcut` 能判定合法完整龙巫组。

两者都不满足时不进入分支比较，直接返回 N 的完整结果。

### D6. 完整周期收益比较

- 分支评价必须使用完整周期收益，不是菲亚目标房间的局部效率；
- 沿用当前 profile 的既有目标（trade/manufacture 加权策略），不新增隐含规则；
- 平局时保留基线 N（不为了形式上的菲亚动作而破坏排班）；
- 其它贸易站正常搜索结果、制造站、发电站、中枢、阶段轮换、菲亚延长工作增益、
  其它岗位占用损失、贸易站数量导致的可行性与机会成本，都通过"各自完整求解后的
  完整报告"自然纳入，不再手工拼装局部字段。

### D7. 上班优先级改写（实现方式，替代预排锁房）

不使用预排 seed、不锁房间、不 refill 覆盖。候选差异只通过两个轻量机制表达：

```text
A 分支：system_preferences 偏好 trade_core 选但书 / 可露希尔
B 分支：不干预，巫恋组靠 L1 shortcut 自然命中
菲亚动作：对固定两人各换一次心情（在各自所在班次写入动作，不改 assignment）
```

- trade 搜索按 `trade_efficiency_sort_key` 总效率降序取 top-K
  （`crates/infra-core/src/search/trade.rs:359`），L1 shortcut 组合效率高自然优先；
- `system_preferences`（`crates/infra-core/src/layout/orchestrate/rules.rs:381`）
  按 `trade_core=<alternative_id>` 指定 alternative 偏好排序
  （`single_trade_closure` / `single_trade_docus_fallback` / `multi_trade_docus` /
  `multi_trade_closure_fallback`），CLI `--prefer` 已支持；
- 无需 `execute_plan` 的 seed 跳过改动、无需 `augment_*_plan` 注入；
- 每个候选各自完整跑一遍现有 pipeline，比较完整周期收益。

### D8. 菲亚梅塔心情的简单实现（只换心情，不钉岗）

菲亚动作在候选各自完整结果上生成，内部模拟菲亚实时心情，且**不改变排班
assignment**（目标在哪个班次在岗，就在哪个班次换心情）：

```text
菲亚梅塔心情回复速度固定（+2/h），攒满后与目标交换心情
```

- 复用现有 `fiammetta_ready_events`（`team_rotation.rs:196`）的模型：
  `ready_mood = (ready_mood + 2.0 * since_event).min(mood_cap)`，攒满后
  `ready_mood = target_mood; target_mood = cap`，与现在行为基本一致；
- 不引入第二套心情常量或实时心情曲线模型；
- 动作只写入 `TeamShiftResult.fiammetta`，不改 assignment 成员，不做跨班 refill；
- A 分支目标：但书一次、可露希尔一次（各换一次心情）；
- B 分支目标：巫恋一次、龙舌兰一次（各换一次心情）；
- 每个候选的动作次数、时机、目标去重仍遵守 `docs/菲亚梅塔使用规则.md`。

## 明确不变

- `fiammetta_8_8_4_4`（8844）、`fiammetta_witch_12_12`、二班 `12/12`、深海
  `7/5/7/5` profile 的既有语义与入口；
- 贸易/制造公式、效率模型、无人机、电力、心情结算 owner；
- Mower 动态换班、完整实时心情曲线、跨周期永续排班、宿舍槽位模拟；
- MAA 输出协议与宿舍输出责任；
- 测试代码（验证走 Rust 最终状态 `--assert-invariants` gate + Python 持久化 box
  批量求解 + Sol 复审，不新增/修改源代码内测试）；
- 工作区中非本会话的其它未提交改动（如 AGENTS.md 技能档位段、pipeline/shortcut
  既有改动等）不属于本任务范围，不清理、不混入。

## 实现计划（逐 stage，每 stage 用本 ADR 复审）

每个 stage 是一个主要意图 + 一次原子提交。每波动手前用一句自然语言报本波目的与
责任边界，获用户批准或明确继续后才进入下一波。

### Stage 1：撤销预排锁房，改为自然选中 + 优先级改写骨架

**目的**：移除预排 seed / refill 覆盖的错误路径，建立"不预排 + 优先级改写 +
只换心情"的骨架。

**范围**：
- 撤销 A 分支的预排 seed 构造与 refill 延续逻辑，A 分支改为不传 seed；
- B 分支改为不预排锁房（靠 L1 shortcut 自然选中），保留资格判断；
- 保留前置资格判断与 `match_witch_group_shortcut` 复用；
- 验证"自然选中"假设在代表性 box 上成立（但书/可露希尔/巫恋组无预排也进站）。

**明确不改**：8844、其它 profile、公式、测试。

**复审点**：候选层不再有预排 seed / refill 覆盖；自然选中在达标 box 上成立；
普通路径行为不变。

### Stage 2：A 分支（trade_core 偏好 + 菲亚两人各换一次心情）

**目的**：A 分支用 `system_preferences` 偏好 trade_core 选但书/可露希尔，菲亚对
两人各换一次心情，收益与 N 自然分离。

**范围**：
- A 候选按 D4 选但书或可露希尔，通过 `system_preferences` 传入候选分支；
- 完整 pipeline 自然生成排班；
- 菲亚动作对但书、可露希尔各写一次换心情动作（不改 assignment）。

**复审点**：A 与 N 的收益自然不同；两人各一次动作；不固定房间槽位、不 refill。

### Stage 3：B 分支（L1 shortcut 自然选中 + 菲亚两人各换一次心情）

**目的**：B 候选完全不干预巫恋组落位，靠 L1 shortcut 自然选中，菲亚对巫恋、
龙舌兰各换一次心情。

**范围**：
- B 候选按 D4/D5 判断资格，不传 seed、不干预搜索；
- 完整 pipeline 自然生成排班；
- 菲亚动作对巫恋、龙舌兰各写一次换心情动作。

**复审点**：B 在拥有且达练度的 box 上巫恋组自然进站；条件不足时正确跳过；
不手搓规则。

### Stage 4：收益比较与收口

**目的**：以完整周期收益选出 winner，收口诊断与最终状态门禁。

**范围**：
- N/A/B 按 D6 比较，winner 胜出，平局保留 N；
- 候选诊断走 stderr，保证 `--json` 结构化输出合法；
- 运行 `cargo fmt`、真实 CLI 入口、`arknights-invariant-gate` 与 Python 批量 box
  验收（Rust 最终 `assignment` 上注册 `--assert-invariants`，Python 只报告 Rust gate、
  无解、输入错误与超时）；
- Sol 复审实际 assertion diff 至 PASS。

**复审点**：完整周期收益比较正确；诊断不污染 stdout；批量 box 不产生非法 assignment。

## 后果

正面后果：

- 候选差异通过轻量机制（trade_core 偏好 + 只换心情）表达，不与 γ 搜索抢房、
  不依赖 seed 传播边界；
- 复用既有 `match_witch_group_shortcut` 与 `fiammetta_ready_events`，不重复造轮子；
- 自然选中已被真实 box 实测验证，排班结构与 N 分支一致，可解释性强。

代价与限制：

- 每候选完整 pipeline 使求解耗时 ×（候选数），需用批量脚本衡量，不影响普通路径；
- 优先级改写只影响 trade_core 的 core 选择，巫恋组完全依赖 shortcut 自然命中，
  不保证每个 box 都出现巫恋站（取决于练度与候选池）；
- 该方案证明的是候选分支内的完整周期收益，不证明全基建全局最优；
- 单贸易站/多贸易站对分支可行性的影响只在完整求解后自然体现，不做人工加权。

## 替代方案

### 预排中间态 + refill 延续（已实现并放弃）

曾用 `execution_seed` + `apply_profile_fiammetta_rotation` refill 表达候选分支。
实测发现 seed 锁房与 γ 搜索抢房、refill 与候选语义前置条件不一致，任何单点修复
都会连锁回归。放弃，改为 D7 的优先级改写 + D8 的只换心情。

### 复杂 Plan 预认领 + anchor 注入（曾部分实现，放弃）

曾用 `augment_witch_profile_plan` + `SystemAnchor` 注入表达候选分支。它难以表达
"只钉人不钉房"，且 A 分支因此退化为"事后找班次"。放弃。

### 局部 refill（旧实现，已被否定）

`refill_trade_room_with_core` / `refill_manu_room_with_core` 不承载菲亚策略选择，
只保留给确实需要"固定核心、局部重填"的其它场景。

### Shadow Candidate 协调器（不采用）

用户已明确本任务是一个薄候选层，不是通用的能力影子候选协调器；`能力影子候选设计.md`
与 `菲亚梅塔Shadow候选收益比较讨论记录.md` 的完整联合收益模型仍为延期 proposal，
不作为本 ADR 的实现范围。

## 与既有裁决的关系

本 ADR 为 `proposed`，暂不改写 current canonical。若接受并实现：

- `docs/菲亚梅塔使用规则.md` 的"放回原房间并替换当前在岗干员"表述需修订为
  "候选分支完整重算 + 对固定目标换心情"，避免误导为局部 refill；
- `docs/SCHEDULE_ROTATION.md`、`docs/定时换班.md` 的菲亚范围需同步到本 ADR；
- `docs/TODO/菲亚梅塔Shadow候选收益比较讨论记录.md` 的第四类问题（已有巫恋组时
  仍选但书组合）由本 ADR 承接为 N/A/B 完整收益比较；
- `docs/TODO/菲亚候选分支实现现状报告与编排教训.md` 记录本轮失败与结构性教训，
  是本 ADR 新方向的输入；
- 其它未被本 ADR 明确替代的排班决策保持有效。

## 复审清单（每 stage 提交前自检）

1. 是否仍遵守"每候选从同一 Box 独立完整 pipeline"？
2. 是否只改写上班优先级、未固定房间/槽位/班次下标、未 refill 覆盖？
3. A 是否自然与 N 收益分离？B 是否靠 L1 shortcut 自然选中且条件不足即放弃？
4. 菲亚动作是否只换心情（写动作不改 assignment）？A/B 各目标是否各一次？
5. 是否复用既有 `match_witch_group_shortcut` 与 `fiammetta_ready_events`，无手搓规则？
6. 是否有针对单 operbox / room id / Shift 下标 / fixture 的补偿特判？
7. 是否遵守"每波一个意图、一次原子提交、波前报目的"？
8. 是否完成风险匹配验证（格式、真实 CLI、invariant gate、批量 box）？

## 背景

### 已确认的原始设计

菲亚梅塔（Fiammetta）在定时换班中的职责是"阶段间的固定策略分支选择器"：

```text
让目标干员获得额外心情 / 工作资格
→ 进入下一阶段的可用候选
→ 下一阶段重新求解完整排班
```

完整流程是：

```text
阶段输入 → 求解本阶段完整 assignment → 菲亚梅塔交接
→ 更新下一阶段输入 → 重新求解下一阶段完整 assignment
```

菲亚梅塔不是：

```text
指定目标原房间 → 清空原房间 → 局部补两个队友
```

### 旧实现为什么产生错误

此前设计明确拒绝回溯重算完整排班，因此实际采用了低成本路径：

```text
已有排班 → 选择菲亚目标 → 只重填目标所在房间 → 尽量不扰动其它 assignment
```

当前代码路径为：

```text
apply_profile_fiammetta_rotation
→ refill_fiammetta_target_room
→ refill_trade_room_with_core / refill_manu_room_with_core
```

这不是偶然实现错误，而是旧约束"禁止回溯重算"的直接结果。服务器真实反馈暴露了该方案的局限：

```text
菲亚前：trade_2 = 卡夫卡 + 巫恋 + 龙舌兰
菲亚后：trade_2 = 涤火杰西卡 + 卡夫卡 + 可露希尔
```

已用服务器真实 ELF 和相同输入确认：γ 阶段组合正确、菲亚处理前的 S2 正确，错误组合在
`apply_profile_fiammetta_rotation` 后产生。

### 本次会话中已撤销的错误实现

本次会话曾提交 `126c2a9`（"普通ABC引入菲亚候选选择层并预冻结完整巫恋站"），
在工作区留有未提交的前置资格判断与复用 `match_witch_group_shortcut` 的改动。经用户
复审确认，该实现是半成品且核心语义错误：

1. **A 候选是"假的"**：A 只跑普通排班后，找到但书本来就在的班次写一个动作标记。
   但书本来就在岗（`trade_core` 已把但书选为持续核心），没有任何"多上一班的延续"，
   因此 A 与 N 的完整周期收益完全相同（`N: trade=3.963 manufacture=8.375 targets=[]`
   对比 `A但书: trade=3.963 manufacture=8.375 targets=["但书"]`）。这违反"让他们一直
   钉在岗位上为前提各分支完全独立"。
2. **B 落位顺序曾被纠正**：B 必须先在 Plan admission 前冻结完整巫恋站（先冻结
   三级金单站 + 巫恋 + 龙舌兰 + 合法裁缝，再让其它岗位围绕它求解），而不是让巫恋
   按搜索规则进驻，等 Plan 先占掉其它 trade core 后被淘汰。
3. **前置资格判断缺失**：分支选择前没有检查用户是否拥有角色并达到练度，导致
   很多 box 上 B 根本不生成。
4. **手搓规则**：曾用"巫恋精二 + β裁缝 + 三级站"手写条件，替代已有的龙巫组合
   判定（`match_witch_group_shortcut`）。
5. **8844 路线收口**：`fiammetta_8_8_4_4` 路线未良好实现，按用户裁决不接入候选流程。

## 决策

### D1. 菲亚梅塔是固定策略分支选择器，不是局部 refill

取消"禁止回溯重算"限制。菲亚梅塔按以下固定流程接入：

```text
同一 Box
  → 前置资格判断（用户拥有角色 + 达到练度）
  → 固定枚举 N / A / B 三候选
      N：不使用菲亚梅塔的完整 pipeline 基线（兜底，参与比较）
      A：但书 / 可露希尔单核心，提前钉岗后完整 pipeline
      B：完整巫恋站（巫恋 + 龙舌兰 + 合法裁缝），先冻结整站后完整 pipeline
  → 每候选独立完整走一遍现有 pipeline，状态隔离
  → 比较完整周期收益（沿用当前 profile 既有目标）→ winner
  → 8844 不收编
```

这不是通用动态回溯框架，而是少量、固定、明确的策略分支各自完整求解。

### D2. 每个候选从同一原始 Box 独立完整求解

- N/A/B 都必须从同一初始 Box 状态开始，各自走完现有完整排班 pipeline；
- 分支之间不能复用可变 assignment：`assignment`、`used`、`teams`、`H1/H2`、
  中枢结果、宿舍结果、阶段轮换状态；
- 可以共享只读或静态数据：`layout`、`operbox`、skill table、operator instances、静态 pool；
- A 分支不能污染 B 分支，B 分支不能污染 A 分支；
- 每个候选独立生成自己的完整排班和切半结果，不能先用一个分支的切半结果再去给另一个分支。

### D3. 候选分支只固定策略组约束，不固定房间和槽位

- 分支只决定"哪几名干员作为策略组必须被使用/钉岗"，不决定具体房间、槽位或班次下标；
- 房间、槽位、队友仍由现有完整排班搜索在剩余范围内决定；
- 不得针对单一 operbox、room id、Shift 下标、fixture 或当前 top hit 写补偿特判。

### D4. 分支定义与降级规则

- **N**：不使用菲亚梅塔，普通完整 pipeline。
- **A**：但书 + 可露希尔单核心策略。按布局降级为一人：
  - 单贸易站 153 布局 → 可露希尔；
  - 其余布局 → 但书；
  - 以候选干员实际拥有情况优先。
- **B**：巫恋 + 龙舌兰 + 合法裁缝完整站（原子组合）。
  - 完整巫恋站条件不足（缺角色、未达练度、无合法裁缝、无三级金单站）时，
    **直接放弃 B 分支**，不允许降级为单人；
  - B 的合法组合判定完全复用既有 `match_witch_group_shortcut`（裁缝 β / α / 白板档位
    与适用练度由该项目统一裁决），不得手搓第二套条件。

### D5. 前置资格判断

进入分支比较之前，先判断"是否有开启分支的必要性"，依据是用户是否拥有角色且达到练度：

- A：存在金单贸易站，且但书任意练度或可露希尔精二至少其一；
- B：存在三级金单贸易站，且 `match_witch_group_shortcut` 能判定合法完整龙巫组。

两者都不满足时不进入分支比较，直接返回 N 的完整结果。

### D6. 完整周期收益比较

- 分支评价必须使用完整周期收益，不是菲亚目标房间的局部效率；
- 沿用当前 profile 的既有目标（trade/manufacture 加权策略），不新增隐含规则；
- 平局时保留基线 N（不为了形式上的菲亚动作而破坏排班）；
- 其它贸易站正常搜索结果、制造站、发电站、中枢、阶段轮换、菲亚延长工作增益、
  其它岗位占用损失、贸易站数量导致的可行性与机会成本，都通过"各自完整求解后的
  完整报告"自然纳入，不再手工拼装局部字段。

### D7. 预排中间态构造（实现方式）

不使用复杂 Plan 预认领/增强（`augment_*_plan` + anchor 注入）作为候选分支的唯一
表达。改为：

```text
构造预排 assignment（中间态）→ 作为 execution_seed 传给现有 pipeline
→ execute_plan 从 seed 起手，保留预排成员 → 搜索在剩余范围补完
```

- 从"从 0 构造"变为"从中间态构造"；
- `assign_shift_with_preclaimed_plan` 已提供 `planning_seed` + `execution_seed`
  双参数（`crates/infra-core/src/layout/assign.rs:283`），`execute_plan`
  从 `seed.clone()` 起手（`orchestrate/execute.rs:31`），seed 成员进入 `used`，
  后续 fill 只在剩余槽位搜索补完；
- 需要让 `execute_plan` 对 seed 中已占用的对应 anchor 不再报错（当前
  `orchestrate/execute.rs:36` 对 `used.contains(anchor.operator)` 直接报错），
  改为跳过已由 seed 满足的 anchor；该改动必须证明不与既有 anchor 语义冲突；
- A 分支：预排"但书/可露希尔在某贸易站"作 seed；B 分支：预排"巫恋+龙舌兰+合法
  裁缝站"作 seed。

### D8. 菲亚梅塔心情的简单实现

菲亚动作在候选各自完整结果上生成，内部模拟菲亚实时心情：

```text
菲亚梅塔心情回复速度固定（+2/h），攒满后与目标交换心情
```

- 复用现有 `fiammetta_ready_events`（`team_rotation.rs:196`）的模型：
  `ready_mood = (ready_mood + 2.0 * since_event).min(mood_cap)`，攒满后
  `ready_mood = target_mood; target_mood = cap`，与现在行为基本一致；
- 不引入第二套心情常量或实时心情曲线模型；
- 每个候选的动作次数、时机、目标去重仍遵守 `docs/菲亚梅塔使用规则.md`。

## 明确不变

- `fiammetta_8_8_4_4`（8844）、`fiammetta_witch_12_12`、二班 `12/12`、深海
  `7/5/7/5` profile 的既有语义与入口；
- 贸易/制造公式、效率模型、无人机、电力、心情结算 owner；
- Mower 动态换班、完整实时心情曲线、跨周期永续排班、宿舍槽位模拟；
- MAA 输出协议与宿舍输出责任；
- 测试代码（验证走 Rust 最终状态 `--assert-invariants` gate + Python 持久化 box
  批量求解 + Sol 复审，不新增/修改源代码内测试）；
- 工作区中非本会话的其它未提交改动（如 AGENTS.md 技能档位段、pipeline/shortcut
  既有改动等）不属于本任务范围，不清理、不混入。

## 实现计划（逐 stage，每 stage 用本 ADR 复审）

每个 stage 是一个主要意图 + 一次原子提交。每波动手前用一句自然语言报本波目的与
责任边界，获用户批准或明确继续后才进入下一波。

### Stage 1：撤销错误候选层，建立预排中间态骨架

**目的**：移除 `126c2a9` 与工作区未提交改动中语义错误的候选层，恢复干净基线，
并为"从中间态构造"建立最小可验证入口。

**范围**：
- 撤销/重写 `schedule_abc_rotation_candidates` 及其调用点的错误分支；
- 让 `execute_plan` 支持"seed 已含成员时跳过对应 anchor"，并用最小用例证明不与
  既有 anchor 冲突；
- 保留前置资格判断与 `match_witch_group_shortcut` 复用（方向正确，按 D5/D4 校正）。

**明确不改**：8844、其它 profile、公式、测试。

**复审点**：候选层不再有"事后找班次写动作"的假 A；中间态 seed 能正确进入完整
pipeline；普通路径行为不变。

### Stage 2：A 分支（但书/可露希尔单核心）

**目的**：让 A 候选真正"钉在岗位上"——在完整 pipeline 前预排单核心，使目标跨班
延续，收益不再与 N 打平。

**范围**：
- A 候选按 D4 选但书或可露希尔，构造预排 seed 作 execution_seed；
- 只固定策略组（单核心），房间/槽位由剩余搜索决定；
- A 分支完整走 pipeline，生成自己的菲亚动作与完整收益。

**复审点**：A 与 N 的收益不再完全相同；目标在动作班次真实多上班；不固定房间槽位。

### Stage 3：B 分支（完整巫恋站冻结）

**目的**：让 B 候选先冻结完整巫恋站再进 pipeline，复用既有龙巫组合判定。

**范围**：
- B 候选按 D4/D5 判断资格，预排"巫恋+龙舌兰+合法裁缝站"作 seed；
- 完整条件不足直接放弃 B，不降级单人；
- B 分支独立完整走 pipeline。

**复审点**：B 在拥有且达练度的 box 上正常生成；条件不足时正确跳过；不手搓规则。

### Stage 4：收益比较与收口

**目的**：以完整周期收益选出 winner，收口诊断与最终状态门禁。

**范围**：
- N/A/B 按 D6 比较，winner 胜出，平局保留 N；
- 候选诊断走 stderr，保证 `--json` 结构化输出合法；
- 运行 `cargo fmt`、真实 CLI 入口、`arknights-invariant-gate` 与 Python 批量 box
  验收（Rust 最终 `assignment` 上注册 `--assert-invariants`，Python 只报告 Rust gate、
  无解、输入错误与超时）；
- Sol 复审实际 assertion diff 至 PASS。

**复审点**：完整周期收益比较正确；诊断不污染 stdout；批量 box 不产生非法 assignment。

## 后果

正面后果：

- 菲亚目标获得真正的跨班持续工作资格，收益差异被完整计算；
- 每个候选从同一 Box 独立完整求解，机会成本完整；
- 复用既有 `match_witch_group_shortcut` 与 `fiammetta_ready_events`，不重复造轮子；
- "从中间态构造"是通用能力，未来其它"预排 + 剩余搜索"场景可复用。

代价与限制：

- 每候选完整 pipeline 使求解耗时 ×（候选数），需用批量脚本衡量，不影响普通路径；
- `execute_plan` 跳过 seed 已含 anchor 需要仔细证明不与既有 anchor 冲突；
- 该方案证明的是候选分支内的完整周期收益，不证明全基建全局最优；
- 单贸易站/多贸易站对分支可行性的影响只在完整求解后自然体现，不做人工加权。

## 替代方案

### 复杂 Plan 预认领 + anchor 注入（曾部分实现，放弃）

曾用 `augment_witch_profile_plan` + `SystemAnchor` 注入表达候选分支。它难以表达
"只钉人不钉房"，且 A 分支因此退化为"事后找班次"，实现与意图脱节。放弃，改为
D7 的预排中间态。

### 局部 refill（旧实现，已被否定）

`refill_trade_room_with_core` / `refill_manu_room_with_core` 不承载菲亚策略选择，
只保留给确实需要"固定核心、局部重填"的其它场景。

### Shadow Candidate 协调器（不采用）

用户已明确本任务是一个薄候选层，不是通用的能力影子候选协调器；`能力影子候选设计.md`
与 `菲亚梅塔Shadow候选收益比较讨论记录.md` 的完整联合收益模型仍为延期 proposal，
不作为本 ADR 的实现范围。

## 与既有裁决的关系

本 ADR 为 `proposed`，暂不改写 current canonical。若接受并实现：

- `docs/菲亚梅塔使用规则.md` 的"放回原房间并替换当前在岗干员"表述需修订为
  "候选分支完整重算"，避免误导为局部 refill；
- `docs/SCHEDULE_ROTATION.md`、`docs/定时换班.md` 的菲亚范围需同步到本 ADR；
- `docs/TODO/菲亚梅塔Shadow候选收益比较讨论记录.md` 的第四类问题（已有巫恋组时
  仍选但书组合）由本 ADR 承接为 N/A/B 完整收益比较；
- 其它未被本 ADR 明确替代的排班决策保持有效。

## 复审清单（每 stage 提交前自检）

1. 是否仍遵守"每候选从同一 Box 独立完整 pipeline"？
2. 是否只固定策略组约束、未固定房间/槽位/班次下标？
3. A 是否真实钉岗（收益不与 N 打平）？B 是否先冻结整站且条件不足即放弃？
4. 是否复用既有 `match_witch_group_shortcut` 与 `fiammetta_ready_events`，无手搓规则？
5. 是否有针对单 operbox / room id / Shift 下标 / fixture 的补偿特判？
6. 是否遵守"每波一个意图、一次原子提交、波前报目的"？
7. 是否完成风险匹配验证（格式、真实 CLI、invariant gate、批量 box）？
