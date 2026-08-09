# ADR 0005: 定时换班语义审计裁决与修复边界

> 文档角色：decision
> 生命周期状态：accepted
> 当前真源：docs/定时换班.md；docs/排班模式.md；docs/Fiammetta.md
> 摘要：保存定时换班连续工作、菲亚执行边界及宿舍、办公室、菲亚目标、调试与无人机偏离的审计裁决

> 历史决策状态：accepted
> 日期：2026-08-07
> 当前规则：[定时换班](../定时换班.md)、[排班模式](../排班模式.md)、[菲亚梅塔](../Fiammetta.md)
> 实现参考：[排班轮换](../SCHEDULE_ROTATION.md)、`crates/infra-core/src/schedule/team_rotation.rs`、`crates/infra-core/src/export/maa.rs`、`crates/infra-cli/src/commands/debug.rs`
> 历史评审：2026-08-07 定时换班符合性审阅（terra 多轴调查 + sol 独立复审 + 用户逐项裁决）

本 ADR 保存已经确认的 C1、C2 以及已接受的 D3-D7 偏离修复决策。它不声明任何修复已经实现或验证；当前领域语义以对应 canonical 为准，当前实现事实以代码、数据和可复现 evidence 为准。

## 背景

定时换班 canonical 由 `docs/定时换班.md`（主）、`docs/排班模式.md`、`docs/Fiammetta.md` 三份文档共同组成。一次只读符合性审阅发现 2 处 canonical 内部冲突和若干实现偏离：

- 完整循环连续工作 hard validation 的实际覆盖范围没有准确写入 canonical。
- 菲亚回宿舍和宿舍一号位被误写成 InfraCalc 可强制输出的责任。
- 深海宿舍的群回选择只按技能数值排序，没有表达 canonical 条件优先级。
- ABC 办公室 cohort 与个人出勤语义混淆。
- 菲亚目标资格、固定顺序、布局条件和多成员服务组混在同一列表中。
- debug bundle 的 wire 名与 CLI 别名使用了错误解析入口。
- MAA export 在没有赤金房时仍自行选择制造房 1。

修复必须维持以下责任边界：

```text
Plan / resolved facts
  -> schedule 选择班次动作和具名策略
  -> export 忠实序列化
  -> MAA / 人工执行实际拖动与定时
```

## 决策

### D1. 连续工作 hard validation 当前限定显式 2/4 班

已裁决。

`docs/定时换班.md` 的当前实现描述改为：

> 当前仅显式二班和四班 profile 按完整循环校验连续工作段，并加入 `0.5h` 安全余量。默认 ABC 只输出 `peak_mood_eta`，尚未执行同等的完整循环 hard validation，因此该输出不构成 ABC 全循环可行性或跨周期永续证明。

该裁决只描述当前验证覆盖范围，不废除定时换班的一般领域规则：

- 用户声明时间后仍应判断连续工作是否可支撑该时长。
- 三班及以上仍应观察下班前完整连续工作时间。
- 高消耗角色无法适配时仍应放弃或调整班次。

`docs/SCHEDULE_ROTATION.md` 中“最终状态统一校验连续工作段”的概括必须同步限定，避免与其 §7.1 冲突。

### D2. 菲亚回宿舍由执行器负责，InfraCalc 不强制槽位

已裁决。本裁决吸收“菲亚本人回宿舍未建模”（原偏离 4），不再作为独立代码偏离。

菲亚使用后的执行结果仍是返回宿舍恢复，但责任分为：

- schedule 决定是否使用、目标和顺序。
- MAA export 只写 `enable`、`target`、`order`。
- MAA 或人工执行者负责目标交换、菲亚返回宿舍和实际拖动。
- 宿舍一号位仅是降低误操作的执行建议，不是 InfraCalc 输出保证。
- InfraCalc 不伪造 MAA 契约中不存在的宿舍槽位字段。

`docs/定时换班.md` §5.4、§8.1 和附录 B 必须采用同一表述；`docs/Fiammetta.md` 同步记录 MAA `Fiammetta` 字段契约。

### D3. 群回宿管采用“条件 policy tier + 实际回复校验”

已接受（原偏离 1）。

群回候选先按以下 policy tier 排序：

```text
流明 E2，且目标恢复段有已解析的自动化组实际在岗
> 塑心 E2，且目标恢复段有已解析的感知信息实际在岗
> 流明 E2
> 隐德来希 E2
> 波登可 E1
> 杜林
> 安比尔 = 夜莺 E2
> 其余合法群回候选按实际技能效果
```

条件必须读取目标恢复段最终 assignment 和其 Plan 的 resolved facts：

- 不以账号拥有某干员作为“体系上班”。
- 不按姓名或 tag 在 schedule 末端重推体系是否激活。
- 若上游尚未提供足够 resolved fact，应扩展 Plan 事实，而不是在宿舍选择器中复制体系判定。

同一候选只进入最高命中的 tier。流明条件未命中时仍可进入普通流明 E2 tier；塑心条件未命中时进入“其余按效果”。policy tier 决定业务顺序，`MoodModel` 继续负责计算真实回复量。最终方案仍必须验证 5h 回复足以覆盖 7h 实际消耗；优先级不能绕过恢复可行性。

### D4. 办公室 cohort 表示上岗资格，不保证普通成员个人上二休一

已接受（原偏离 2）。

ABC 三班中：

- 每个办公室候选只归属一个 α、β 或 γ cohort。
- 候选仅在所属 cohort 活跃的两个班次具备上岗资格。
- 每班从两个活跃 cohort 的合法候选中按办公室策略选人并满编。
- 普通成员不保证个人工作两班；实际出勤受办公室席位约束。
- 绑定办公室成员继续随消费方体系严格同上同下；其休息班由合法候选补位。

删除按 Shift 下标指定唯一 `preferred_team` 的路径。Shift 下标不能成为办公室选人语义。

理由是容量守恒：容量为一的办公室在三个班次中只有三个 seat-shift，无法让三个 cohort 成员各占两个 seat-shift。若未来要求每名普通办公室成员个人上二休一，必须设计跨多个循环变化的长周期状态，不能由当前三状态循环表达。

测试应证明：每班办公室满编；休息 cohort 的普通成员不返岗；两个活跃 cohort 均是合法候选来源；绑定成员与消费方 presence 完全一致。

### D5. `continuous_roles` 不承担菲亚优先级；成组目标当前不可服务则放弃

已接受（原偏离 3）。

`ContinuousRole` 只表达已经由 Plan 解析的持续岗位事实（operator + facility + room_id）。其数组顺序不具有菲亚业务优先级。菲亚策略层必须独立构造具名服务目标，并按 canonical 布局条件排序：

```text
但书（二发电，一级或二级贸易站）
> 巫恋 + 龙舌兰（三级贸易站）
> 但书（三级贸易站）
> 清流（三贸易站）
> 可露希尔
```

当前轻量固定顺序只能作为上述完整策略尚未实现时的显式兼容 policy，不能由 orchestration alternative 的首个命中顺序隐式产生。

单人目标必须同时满足：存在于最终 peak 生产 assignment；目标设施和房间仍合法；菲亚回岗不会破坏 owning Plan 的 hard anchor 或 resolved dependency。

“巫恋 + 龙舌兰”是一个原子服务目标组，不是两个互相独立的姓名项。通用 `shift_bind` 只证明同上同下，不能代替该组是否是菲亚候选、在优先级中的位置、同一完整循环需要几个菲亚事件、每个事件的合法时间和目标。当前单事件模型无法在同一完整循环中合法服务两人，因此该组在当前 profile 下不可服务：必须给出可观察原因并继续下一个优先级，不得只让一名成员回岗破坏组合，也不得因属于多成员 bind 而静默吞掉该优先级。完整实现成组服务需要先定义同周期多个菲亚事件及心情就绪条件，作为后续开放项，不并入本次修复。

### D6. debug rerun 按来源区分 wire 解析和 CLI 解析

已接受（原偏离 5）。

解析责任为：

- 用户显式传入 `--rotation`：使用 `TimedRotationProfile::from_str`，接受 CLI 名和 CLI 别名。
- debug bundle 中录制的 serve request：使用 serde 反序列化，接受协议名。
- 历史 bundle 如需兼容其他词汇，必须有显式版本或兼容分支，不得静默把两套词汇混成一个无边界解析器。

测试覆盖四种 wire 名（`abc_12_6_6`、`main_backup_12_12`、`fiammetta_8_8_4_4`、`abyssal_7_5_7_5`）与四种 CLI 入口（`3`、`2`、`fiammetta-8844`、`abyssal-7575`）。录制请求未显式覆盖时，四种 profile 都必须可 rerun，并保留 `origin = recorded_request`。

### D7. 无人机目标由 schedule 决定，export 只序列化

已接受（原偏离 6）。

schedule 或 plan 层生成明确的无人机动作（enabled + target room_id + order）。MAA export 只负责把已选内部 `room_id` 转换为 MAA 的 `room/index`、忠实写出 `enable` 和 `order`、在菲亚-only transition 按 schedule 动作关闭无人机。

默认 policy 只选择实际产品为赤金的制造房。不存在合法赤金制造房时：

```text
enable = false
```

不得：`unwrap_or(1)` 回退第一制造房；指向合成玉制造房；由 export 临时改选作战记录或贸易站；在没有用户资源 policy 时自行决定其他投向。

钱、书、赤金供需和贸易乘区的完整动态选型，待缺失的 `docs/DRONES.md` 恢复或重新裁决后实施。`docs/定时换班.md` 对 `docs/DRONES.md` 的引用当前缺失，恢复该 canonical 前本 ADR 只裁决“不加速合成玉”和默认赤金目标安全边界。

## 后果

### 实现影响

实施应拆为以下责任单元，每个单元只处理自己的主要不变量，不得用干员名、房号、Shift 下标、当前 fixture 或 top hit 为单一样例补偿：

1. 文档单元：同步 D1、D2，并澄清 `SCHEDULE_ROTATION.md` 的实现范围。
2. 调试协议单元：修复 debug bundle wire 解析并补四 profile 回归。
3. 无人机单元：把目标选择移出 export；无合法赤金目标时关闭。
4. 宿舍策略单元：建立条件 policy tier，消费 resolved facts，并保留真实回复量验证。
5. 办公室单元：移除 Shift 下标偏好，按两个活跃 cohort 选人。
6. 菲亚策略单元：分离资格、优先级和服务事件；成组目标在事件模型明确后实施（本次只做优先级显式化与不可服务放弃）。

### 文档影响

必须更新：

- `docs/定时换班.md`：D1、D2，以及用户接受后的办公室、宿舍、菲亚和无人机口径。
- `docs/Fiammetta.md`：区分 `continuous_roles`、目标排序、成组服务和当前事件数量限制。
- `docs/SCHEDULE_ROTATION.md`：只记录实现真实覆盖，不扩大 canonical。
- `docs/FRONTEND_CLI.md`：无人机 disabled 行为和实际字段契约。

ADR 不取代上述 canonical 的当前语义。

### 测试影响

后续实施至少需要：

- 显式 2/4 班完整连续工作 `+0.5h` 拒绝边界；ABC 只展示 ETA，不能被测试描述为同等 hard proof。
- 菲亚 MAA 只有 `enable/target/order`，不伪造宿舍槽位。
- 宿管在条件命中与不命中时切换 tier，并用真实回复量拒绝不足方案。
- 一席及多席办公室证明容量守恒、满编、休息 cohort 排除和绑定 presence。
- 非 243、单贸易、多贸易、三级贸易及三贸易布局的菲亚目标顺序；巫恋 + 龙舌兰不可服务时有明确结果且不破坏组合。
- 四种录制 profile 均可 debug rerun。
- 无赤金房、仅合成玉房、赤金不在制造房 1 时无人机输出正确。
- 至少一次真实 `plan` 和 MAA 导出验证 schedule/export 一致。

### 保留限制

- ABC 的 `peak_mood_eta` 不是完整循环可行性证明。
- 当前文档没有完整无人机动态选型 canonical；`docs/DRONES.md` 待恢复。
- 菲亚多目标组需要事件时序和实时心情能力；在此之前不能声称完整实现动态优先级。
- 宿舍条件必须等待 resolved facts；不能以末端姓名扫描替代。
- 本 ADR 不声明任何代码修复或测试已经完成。

## 已放弃的替代方案

### 把 C1 写成 ABC 无需连续工作验证

放弃。C1 只限定当前 hard validation 覆盖，不能削弱定时换班一般可行性规则。

### 让 InfraCalc 强制菲亚宿舍一号位

放弃。MAA 字段无法承载该槽位，实际拖动属于执行器。

### 群回宿管只按技能数值排序

放弃。该方案会让杜林数值覆盖 canonical 条件优先级，并遗漏流明、塑心的上下文条件。

### 在三状态循环中要求每名办公室成员上两班

放弃。该要求在一席办公室中违反 seat-shift 容量守恒。

### 用 `continuous_roles` 数组顺序作为菲亚优先级

放弃。数组顺序来自 Plan 编译过程，不是排班业务 policy。

### 让通用 `shift_bind` 决定菲亚成组目标

放弃。bind 只负责 presence，不能表达优先级、服务次数和事件时序。

### 把所有 serde 名加入 CLI `FromStr`

放弃作为主要修复。录制请求和 CLI 输入属于不同协议来源，应使用各自解析器。

### 无赤金房时回退制造房 1

放弃。该路径可能加速合成玉订单，并让 export 成为无人机选型 owner。
