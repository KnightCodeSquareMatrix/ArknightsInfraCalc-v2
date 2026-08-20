# ADR 0007: 回退恢复后的中枢与二发电事实边界

> 文档角色：decision
> 生命周期状态：accepted
> 当前真源：docs/CONTROL_CENTER_ASSIGNMENT.md；docs/公孙长乐的体系分析文档/ROSEMARY_PERCEPTION_CHAIN.md；docs/公孙长乐的体系分析文档/AUTOMATION_GROUP_CHAIN.md
> 摘要：恢复历史回退后中枢候选、动态 producer 与二发电宿舍数值的责任边界

> 日期：2026-08-18
> 状态：accepted with D1 implementation gate；实现与验证尚未完成
> 实现参考：`crates/infra-core/src/search/control.rs`、`crates/infra-core/src/pool/control.rs`、`crates/infra-core/src/layout/blueprint.rs`、`crates/infra-core/src/layout/assign/control_fill.rs`、`crates/infra-core/src/schedule/team_rotation.rs`、`crates/infra-core/src/global_resource/inject.rs`、前端 layout wire contract
> 历史背景：7/30 回退到 7/19 的部署事实待从服务器 release 元数据补证；旧主站 binary 仅作为行为基线，不作为 Git 真源

## 背景

专家批量审阅发现中枢重复、无收益候选进入中枢、凛御银灰误激活，以及二发电空弦/锡人被高估。代码审阅显示这些问题横跨准入、候选排序、结算和轮换，不能用单个干员名单或当前 top hit 特判修复。

当前已确认的历史路径：

- `7425b08` 的分层补位评分按候选人数累计心情分，而真实同类心情结算取最高。
- `3aceaab` 引入无技能合法干员 filler，`2b73ae7` 扩展为稳定全量 filler 尾部。
- `16bf4bd` 允许 `dorm_beds` 回退为宿舍技能有效等级；252/342 未提供 ambience 时被算成总等级 20。
- `b822c82` 将锡人办公室评分接入该共享 layout context。
- `f269482`、`86d075f`、`a0aae8`、`1814438` 属于 7/19 之后的局部恢复/抑制修复，可能被部署回退丢失。

## 决策

### D1. 宿舍有效等级由前端事实提供

目标 contract 是由前端作为宿舍有效等级的事实 producer，并为每间宿舍提供 `dorm_ambience_level`；`RoomBlueprint` parser 是该字段的 wire validator，核心只消费已验证字段计算 `dorm_level_sum`。该 contract 尚未在前端 schema 和 parser 中落地，因此 D1 目前是实施门，不是当前已实现事实。

目标语义是：`dorm_beds` 只表示床位容量，不再作为“每间宿舍每级”技能的语义回退。完成前端 schema、wire producer 和 parser validator 后，252/342 缺少任一宿舍 `dorm_ambience_level` 必须被拒绝；当前仍存在的 beds/level fallback 记录为已知违规。其他 legacy layout 的兼容语义另立决策，不在本 ADR 中扩张。

本 ADR 不裁决 252/342 各自具体的宿舍等级；具体值由前端事实和对应 canonical 输入提供。

### D2. 凛御银灰与银灰保持独立身份和 owner

凛御银灰是独立的保留 producer rule：只有最终存在同一贸易站至少三名谢拉格成员时才有 consumer 阈值事实，但自动中枢 admission 为 `excluded_automatic`，不得进入 producer presence、普通 filler 或轮换 fallback。

银灰是另一名干员，不得与凛御银灰共享 producer buff、required anchor、rotation bind 或 identity 特判；但银灰作为带 `cc.g.karlan` tag 的贸易站成员必须计入凛御银灰的消费者阈值。

动态 producer 的事实由 Plan/response dependency 解析；rotation 只消费 resolved facts，不按姓名或 tag 重新激活。银灰仍可作为独立 `cc.g.karlan` consumer 参与阈值事实，但不会因此放行凛御银灰。

### D3. 中枢同族效果按有效贡献结算和候选效用排序

同一 inject family 的数值合并由 `GlobalInjectManifest` 取最大，跨 family 才相加。candidate utility 消费该 settlement 的 winner/suppressed 事实；被 `Suppressed` 的 source 不产生有效产能或 replacement tie-break 优先级。上限截断或替代的 source 是否仍有部分有效贡献，必须由 manifest 的最终 merge/trace 逐项判定，不能一律归零。

候选排序不得按“每个携带该技能的干员固定加分”。候选组合的补位价值必须按加入该组合后的有效边际贡献计算；维什戴尔与玛恩纳的边际价值须按目标设施、班次、全局回复取最高与玛恩纳 smiley 扩散分别计算，不得因人数重复获得全局回复排序收益。

这不要求把所有同族干员硬排除出 5 人组合；family admission、最终 settlement 和机会成本排序必须由同一事实 owner 协调，不能由房间顺序或姓名规则代替。

### D4. 无技能 filler 只作为有界最后补位

中枢应优先满足 required system、有效产能/解锁入口、体系外有效散件和心情补位；仅在仍需填满合法容量时使用无技能普通干员。无技能 filler 不得抢占前述有效候选，也不得扩大候选搜索到无法证明的全量组合。

凛御银灰的动态 producer 路径不得降级为普通 filler；自动 admission 关闭其所有中枢候选路径。普通银灰仍按其自身机制和有效贡献处理。

### D5. 版本恢复采用行为基线和可追溯 bundle

旧主站 binary 只作为行为 baseline 保存；每个新 solver bundle 必须原子记录 executable hash、data manifest hash、baked manifest hash、plan contract/schema、Git commit（若可用）、构建时间、实际安装路径和运行时解析后的身份。回滚必须保存旧 tuple、触发条件和回滚后同一矩阵复验。

回退恢复必须通过同一输入的 differential matrix 验证，至少覆盖中枢、252/342、空弦、锡人、凛御银灰和反馈中的真实 Box。不能因为版本号更新或 health 通过就声明修复恢复。

## 单一责任边界

| 不变量 | owner | 禁止的冲突路径 |
|---|---|---|
| 宿舍有效等级 | **目标**：前端事实 producer + wire schema + `RoomBlueprint` parser + `blueprint.rs` 消费；当前 owner gate 未完成 | `dorm_beds` 作为技能等级回退、空弦/锡人姓名特判 |
| 凛御银灰动态激活 | Plan/response dependency | rotation 按 tag/姓名重推、无 consumer 时伪造动态收益 |
| family max 与 suppressed source | `GlobalInjectManifest` settlement + candidate utility | 每人固定加分、房间顺序、当前 top hit 特判 |
| filler 机会成本 | control admission/fill policy | 全量无技能候选尾部抢占有效候选 |
| 版本身份 | bundle manifest/deployment evidence | 只看服务健康、只看 frontend commit、无 hash 的 binary 替换 |

## 验证要求

- **data contract hard constraint（D1 gate）**：完成前端 schema/wire/parser 后，252/342 每间宿舍都携带精确 `dorm_ambience_level`；缺失字段被 parser 拒绝；`dorm_level_sum` 等于该四项之和而非 beds 总和。当前 contract 尚未满足此断言，不得报告为已通过。
- **mechanism hard constraint**：空弦和锡人分别在前端给定的实际等级输入与合成 `20` 输入下验证公式值不同；只前者可作为 252/342 排班输入。
- **settlement hard constraint**：维什戴尔+玛恩纳按目标设施取最高全局回复，smiley 扩散独立断言；同 family source 验证 manifest winner/suppressed。
- **candidate policy/heuristic**：中枢 utility 只计算边际有效收益；分别覆盖少于五人、恰好五人、有效候选竞争和 filler fallback；`ControlInjectRawSumV0` 与 `TradeManufactureWeightedEfficiencyV1` 分开报告。
- **dynamic producer hard constraint**：银灰可计入谢拉格 consumer 阈值；凛御银灰自动 admission 排除，不生成 producer bind；consumer 阈值仍由独立规则事实验证。
- **safe-reduction proof**：关闭/开启 skillless tail pruning 的完整候选差分不得改变有效候选胜者；只允许改变等价 filler 身份。
- **deployment evidence**：对旧主站 baseline、7/19 tree、当前 tree、当前 bundle 执行同一 feedback matrix differential，并记录完整 deployment tuple。
- 实际 `plan`、`layout team-rotation`、MAA export 和 runner bundle identity 均需留 evidence。

## 未决与非目标

- 本 ADR 不裁决 252/342 具体 ambience 数值。
- 不恢复通用跨站 producer 求解器，不新增干员名补偿特判。
- 不把旧 binary 的行为自动当作领域真理；差异必须回溯到 canonical、代码 owner 或数据事实。
- 旧 Box 的非法 rarity/elite 兼容策略另立修复单元，不与中枢/二发电修复混写。
- `worker-json` 的单账号 Skland session 协议升级是独立认证/产品迁移；只要其布局 suggestion 在二发电时传递 `dorm_ambience_level`，该协议差异不作为 D1 宿舍数值修复的验收条件。
