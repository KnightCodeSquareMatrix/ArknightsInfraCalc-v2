# 排班轮换（Schedule Rotation）

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/排班模式.md；docs/定时换班.md；docs/菲亚梅塔使用规则.md
> 摘要：记录当前定时换班 profiles、ABC 默认实现和入口

> **当前实现参考**：`schedule_timed_rotation` 支持默认 αβγ `12/6/6`、ABC `12/12/12`、普通二班 `12/12`、菲亚龙巫二班 `12/12`、菲亚 `8/8/4/4` 和深海 `7/5/7/5`；`schedule_team_rotation` 保留为默认 ABC 薄包装。Agent 默认经 `plan` 触发，仅排班入口为 `layout team-rotation`。模式边界由 [排班模式](排班模式.md) 裁决，定时换班规则由 [定时换班](定时换班.md) 裁决。A-B-A 的 CLI、core API 与 MAA 导出已移除。

> 动态 producer 的统一搜索与 resolved dependency 见 [CONTROL_CENTER_ASSIGNMENT.md](CONTROL_CENTER_ASSIGNMENT.md)；完整条件响应行与 Bake 加速继续由 [A+ TODO](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md) 跟踪。

---

## 1. 当前模型

| profile | CLI | 状态与 owner | 失败语义 |
|---|---|---|---|
| `abc_12_6_6` | 省略 `--rotation` 或传 `3` | 现有 αβγ ABC；一个 peak Plan 派生三态 | 保持现有行为 |
| `abc_12_12_12` | `--rotation 12/12/12` 或 `abc-121212` | 复用 ABC 三队结构，三段各 12h，总周期 36h；菲亚次数与时机见 [菲亚梅塔使用规则](菲亚梅塔使用规则.md)，不启用深海短班 | 沿用 ABC 编排与既有机制约束 |
| `main_backup_12_12` | `--rotation 2` | 主替完整 Peak 求解（main 优先填充，再以最终 main 排除边界求 backup），各自保留 `AssignmentPlan`；主替 assignment 全成员互斥；办公室按主/替上 1 休 1 | 互斥 policy 下无解即报错，不回退；办公室/会客室缺员仅 warnings |
| `fiammetta_witch_12_12` | `--rotation fiammetta-witch-1212` | 两个预认领 Plan owner 共享巫恋+龙舌兰，同一三级赤金贸易站内由两只不同裁缝 β 按 AB 轮换；每班一个菲亚目标 | 缺菲亚、完整龙巫、两只裁缝 β、合法房间或完整 owner 即报错；不回退普通二班 |
| `fiammetta_8_8_4_4` | `--rotation fiammetta-8844` | ABC `16/4/4` 展开首态；第二个 8h 是具名菲亚事件 transition | 缺菲亚或合法目标即报错；办公室/会客室缺员随 ABC 语义 warnings |
| `abyssal_7_5_7_5` | `--rotation abyssal-7575` | profile required Plan 锚定一个能力 producer 与四名 tag consumer；main 填充后独立替补交替两次，5h 恢复态固定 dorm_rest | hard claims 或替补不可满足即报错；办公室/会客室缺员仅 warnings |

所有显式 profile 都是 hard constraint，不静默降级。显式 2/4 班最终状态统一校验生产房容量、中枢 5 人、profile 自有 hard anchors、owner-scoped dependency、Team presence、连续工作段与 `0.5h` 余量；普通二班的 Team 仍全员互斥，只有 `fiammetta_witch_12_12` 具名允许巫恋、龙舌兰属于两个 Team 并跨班连续工作。默认 ABC 校验生产房容量、中枢 5 人、显式 bind 与 mood-gap 12h 限制，但只展示 `peak_mood_eta`，不执行完整循环连续工作段 hard validation。普通 Plan anchors 已由各自 `execute_plan` 在 assignment 构造时验证，不在 rotation 终验重复宣称。搜索仍是现有编排/设施 policy，不宣称全局最优。`TeamRotationReport.assignment_plans` 保留每套独立 assignment 的 Plan，`shifts[].plan_index` 声明 owner；`peak_plan` 仅保留兼容主力别名。

`DailyTotals.production` 对传入的全部 `shifts` 做时长加权，并按总周期归一到 24h：
`sum(产出率 × 班次时长) / sum(班次时长) × 24`。因此汇总器本身不依赖固定班次数量或固定时长；但当前 `TimedRotationProfile` 仍是闭集，CLI 和 `schedule_timed_rotation` 尚不能接受任意自定义班次数组。当前六个已落地 profile 均复用该汇总路径。

办公室普通未绑定成员按效率优先逐班选择：每个班次在生产与其他固定岗位落位后，从当班剩余候选中按办公室效率从高到低尝试；只检查当前班次 ETA，上一班是否使用不参与办公室普通候选排序或准入。`12/6/6` 和 `12/12/12` 都不要求普通办公室成员固定复用，也不把办公室成员加入生产 αβγ cohort。地灵 E1 等高消耗角色在当前班次 ETA 不足时跳过，ETA 足够的短班仍可入选。会客室继续按自身既有轮换和心情规则处理。真实绑定成员仍优先服从 `shift_bind`，不被普通办公室候选逻辑拆开；若绑定成员当前班 ETA 不足，该绑定状态直接不可行，不以普通候选单独替换。

办公室与会客室不要求「必须满编」：候选不足可空置，但实际人数不得超过容量，缺额必须经 `TeamRotationReport.warnings`（及 serve `rotation.warnings`）逐班提示（shift、设施、room id、缺额）。roster 充足时仍须满编且 warnings 为空。该容错覆盖 ABC、二班、菲亚与深海全部 profile，当前实现以本页和最终状态校验为准。

MAA `plans[]` 与最终状态一一对应，顶层 `planTimes` 为 `2班` / `3班` / `4班`。每个 plan 说明下一段相对间隔；自动定时触发仍由外部 MAA 任务配置。菲亚事件态只重放目标房，其他房间和无人机均跳过。

深海 5h 恢复态额外携带 `dorm_rest`：schedule 从当班未在宿舍外工作的已拥有干员中分别选择单回与群回宿管，和深海中枢 producer 固定到同一宿舍，按“单回、群回、目标”顺序计算并导出；5h 回复量不足 7h 实际消耗时 profile 硬失败。

### 1.1 默认 ABC

| 项 | αβγ ABC |
|---|---|
| CLI | `plan`（默认，含账号分析）/ `layout team-rotation`（仅排班） |
| 核心 API | `schedule_team_rotation` |
| 班次结构 | 12h + 6h + 6h；每班 **两队上岗、一队休息** |
| 生产设施 | 切成 H1/H2 两半；α/β 来自 peak 切半，γ 替补 |
| 中枢 / 宿舍 / 办公室 | 办公室不属于生产切半或 αβγ cohort；每班生产/固定岗位完成后，从当班剩余候选按效率和当前 ETA 独立选择，上一班办公室成员不提供普通优先级；真实绑定成员仍随消费方体系同上同下；中枢仅在 ABC profile 中按活跃两队轮休重分配，每班补满 5 人；`main_backup_12_12` 的普通日周期中枢独立生成两套 12h 状态，不参与生产切半；宿舍暂保持共享 |
| 设施空转 | 生产/中枢 **禁止**（每班满编）；办公室/会客室候选不足可空置，但必须经报告级 `warnings` 显式提示缺额 |

用户说「跑一遍模拟」「三班模拟」时，Agent 默认用 **`plan`** + `--maa-out`（账号分析 + αβγ 排班）；只有用户明确要求“仅排班”时才用 **`layout team-rotation`**。见 [INFRA_CLI.md](INFRA_CLI.md)。

---

## 2. ABC 轮换流程

```
peak = assign_shift_with_plan(Peak) → { assignment, plan }
shared = pinned_assignment_excluding(peak, all_office_ops)
                                       # 仅宿舍共享；办公室不钉死
[h1, h2] = split_production_facilities  # exact-bind 闭包不可拆；贸易闭包优先分半
align_shift_binds(h1, h2)            # 迷迭香+黑键等同队
α = peak ∩ h1,  β = peak ∩ h2
γ = assign_team_gamma_half(h1) + assign_team_gamma_half(h2)  # 贸易走 docus/closure/witch/witch_fallback/karlan/penguin/plain
team_ctrl = build_team_control_map(peak.control, plan, h1)    # 中枢干员归入 αβγ
team_ctrl += core inject / hr-mood control candidates         # 效率注入/公招心情散件

S1 (12h): shared + control(α+β) + α(H1) + β(H2)   休息 γ
S2 (6h):  shared + control(β+γ) + β(H2) + γ(H1)   休息 α
S3 (6h):  shared + control(γ+α) + γ(H2) + α(H1)   休息 β

菲亚覆盖：三班组装完成后，从 peak 主力按确认优先级选择一人，在其所属队伍
原本休息的班次中放回原房间，并换下一个当前在岗干员。
```

γ 替补贸易与 peak `assign_trade_remainder` 同路径。金单顺序是
`docus → closure → witch → witch_fallback → karlan → penguin`，再 plain；格拉斯哥是普通合法
候选，不占固定 role；制造/发电仍站绑定贪心。但书优先低等级金单站，可露希尔优先剩余
高等级金单站，完整龙巫只进入三级站。

实现：`crates/infra-core/src/schedule/team_rotation.rs`。

---

## 3. 中枢轮休规则

### 3.1 普通日周期二班

`main_backup_12_12` 下，普通中枢使用两个固定的 12h 状态，不复用 ABC 的生产切半和 αβγ 中枢轮休逻辑。主状态与替状态各自独立求解、各自填满 5 个合法席位，并遵守二班完整状态互斥策略。

中枢效果账本在每个 12h 状态内独立结算。同一种全局效果只允许其 `Winner` / `TiedWinner` 来源获得该状态的有效候选价值；`Suppressed` 来源不得与 winner 同班。因此，阿米娅和阿斯卡纶可以分别承担两个状态的贸易全局效果，但不能同时进入同一个 12h 中枢状态。

该模型不适用于歌蕾蒂娅深海心情方案。歌蕾蒂娅作为中枢 producer 时，按 canonical 的 `abyssal_7_5_7_5` 单独处理，不因普通中枢规则改成两班。

### 3.2 ABC 中枢轮休

仅对 `abc_12_6_6` / `abc_12_12_12` 保留按队伍轮休规则，而不是三班钉死同一套 5 人。目标：

- 每班中枢满编 5 人。
- 休息队的中枢干员不进入当班中枢。
- 每个中枢干员在 αβγ 周期内至少休息 1 班。
- 优先保留贸易 / 制造效率注入；未被体系认领的纯资源型 standalone producer 不作为中枢插件补位，已认领或具备插件能力的动态 producer 仍可经普通中枢候选/插件位按实际计划结算。

中枢干员归队：

1. 体系绑定中枢位跟随其生产体系所在半区：H1 → α，H2 → β；纯中枢体系默认 α。
2. peak 中枢里的非体系散件按当前队伍人数最少优先均分到 α/β/γ。
3. 额外中枢候选只收两类：核心贸易/制造注入，或公招/心情回复类补位。
   未被 `base_systems` 认领的状态 producer（热情、木天蓼、情报储备等）不作为普通插件放入中枢。
4. 若额外候选已经在生产队中，跟随该生产队；否则分到当前中枢人数最少的队。

每个班次只用活跃两队的中枢候选建池。体系中枢位先 pin 到 `control`，再由 `assign_control` 补满 5 人。轮换内设置 `skip_standalone_control = true` 以保留体系 pin，但补位仍按 plugin 规则过滤；排序分为 `ControlInjectRawSumV0` 注入分量（`trade_inject + manu_gold + manu_br`）加公招/心情补位分。这是中枢候选的局部排序 policy，不是贸易/制造平衡公式。

S2 有深海短班特例：运行时按稳定 control buff capability 找歌蕾蒂娅 producer，按 `cc.g.abyssal` tag 找制造 consumer；歌蕾蒂娅加至少 3 名其他深海猎人即可启用，满足门槛的深海候选优先于普通 S2，不再以普通 S2 制造效率比较决定是否采用，且不进入 12h 主班。已有的额外深海猎人可继续进制造站；候选只枚举“每个制造站放几人”（单站 0–3），不枚举具体排列或原房间成员子集；补中枢只保护调用方传入的 producer pin，不按名字保护。

相关单测：

- `team_rotation_control_center_rest_rotates`
- `team_rotation_control_center_respects_resting_team`
- `team_rotation_control_prefers_trade_manu_inject_over_resource_only_fillers`
- `team_rotation_abyssal_only_runs_in_s2_short_shift`

---

## 4. 243 贸易 core role

243 双贸中，贸易站不再依赖 `blackkey_closure` / `witch_long_beta` fixed registry 早占房。peak 与 γ 替补都走同一条 role 顺序：

八幡海铃、戴菲恩由同一次 producer presence 枚举自然入选，并由 winner 的实际 consumer 派生 resolved dependency；凛御银灰 producer rule 仅保留 consumer 阈值事实，自动中枢 admission 排除；不存在 `meta_vina` role。房内完整响应行 join 仍见 [A+ 实施 TODO](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

最新 canonical 是：多贸龙门币默认但书 > 可露希尔 > 完整龙巫；Rosemary/黑键 alternative 激活时，
Plan 进一步固定但书 A + 可露希尔/黑键 B，并把完整龙巫 C 交给 γ reserve。下面编号记录当前
代码入口、房间等级偏好和 required 条件。

1. `docus`：精0/精二但书均可在金单站成为核心；蓝图实际有至少 2 间贸易站且至少 1 间为龙门币订单时，作为首个 required core；全源石单不启用。anchor 优先解析到最低等级的可用金单站，再从所有可用干员中按最终效率选择队友。叙拉古不注册固定体系：八幡海铃、伺夜、贝洛内均可不入选，伺夜与贝洛内可单走、同上且跨站、或自然同站；包含八幡海铃的中枢候选只有在最终实际贸易成员使其动态收益胜出时才采用。shortcut 仅按最终实际同房组合自然结算。
2. `closure`：蓝图实际恰有 1 间贸易站且为龙门币订单时是 required core；多贸时排在但书之后、完整龙巫之前，并优先剩余的最高等级金单站。完整 Rosemary/黑键 alternative 激活时则是该双贸 alternative 的条件 required core。有黑键时可命中 `gsl_blackkey_closure`，缺黑键时仍保留可露希尔。
3. `witch`：多贸龙门币默认第三核心；必须由精二巫恋 + 已持有的龙舌兰组成并进入三级金单站。龙舌兰 `tier_up` 时优先使用裁缝 β/α；龙舌兰 `tier_0` 允许 β/α/白板三档 `gsl_witch_long0_*` 的低练度排班路径。普通白板和缺少龙舌兰的二人组不得进入自动龙巫站。
4. `witch_fallback`：无龙舌兰时的巫恋兜底。
5. `karlan` / `penguin` / plain：灵知孑、企鹅、散件工具人；格拉斯哥成员在这些正常候选中按实际 `total_efficiency` 竞争。

双贸易站中 Rosemary/黑键已激活，且但书、可露希尔和完整龙巫均可形成时，三个贸易 cohort 定义为：

| cohort | 贸易站 | 成员约束 |
|--------|--------|----------|
| A | 但书核心站 | 但书 + 正式效率搜索选中的两名队友；full-E2 期望伺夜+贝洛内 |
| B | Rosemary 黑键站 | 可露希尔 + 黑键 + 正式搜索第三人 |
| C | 自动龙巫站 | 精二巫恋 + 持有龙舌兰；`tier_up` 龙舌兰优先 β/α，`tier_0` 龙舌兰可走 long0 β/α/blank fallback |

菲亚后处理之前的基础 rotation 必须容纳三组，组合为 `A+B → B+C → C+A`；房号、H1/H2、αβγ 标签不固定。可露希尔与黑键同组是该完整 Rosemary alternative 的声明式 packing 结果，不是全局“黑键固定可露”；B 的第三人和 A 的叙拉古成员仍由正式搜索决定，C 仍只接受合法裁缝。A/B 若被已有 exact bind 连成不可分分量，或自然 H1/H2 打包不能让两边各有目标，计划阶段事务性降级，不得到 schedule 再搬房或报错。缺可露、缺完整龙巫或 Rosemary 未激活时同样分别降级，不得把降级态伪报成完整三组。单测：

- `team_rotation_binds_only_actual_syracusa_members`
- `team_rotation_partial_trade_meta_keeps_docus_closure_and_complete_witch`
- `team_rotation_full_e2_resolves_gamma_cohort_before_postprocess`

`ResolvedRoleReserve.reuse_policy` 是基础 rotation 的通用轮换语义：`once` 只在 H1/H2 中首个可行目标房使用一次；`every_eligible_half` 则要求两个 half 都存在 eligible room，并在 `apply_fiammetta_return` 前把同一组已解析成员分别写入两个 γ half。带 `require_pre_split_halves` 的 pack 在 plan 提交前先验证自然切半，失败即整包降级；schedule 只校验已经提交的 reserve 未缺目标、未被基础 fill 覆盖。reserve 不保护菲亚后处理后的最终房间成员。

暖机干员的“房间稳定”仍只约束其连续上岗时不跨房；基础三 cohort 状态额外要求 C 作为同一 γ 替补组复用于 A、B 各自休息的班次。实际叙拉古 producer dependency 只能绑定 A 中真正贡献的成员，不能把 B 或 C 并入该 bind，也不能借此把 A/B 两个贸易房合并成同一 cohort。

---

## 5. 班次绑定（shift_bind）

部分干员须 **同上同下、上 N 休 M**，在 schedule 层处理（非编排层、非 global effect）。

资源转换链遵循相同的实际 presence 原则：provider 与 converter 同班实际工作时转换才激活；
这不是 required admission，也不要求同房。当前同一 buff 自闭合链无需新增 bind；跨干员链必须
由联合 winner 返回实际 provider/converter dependency 后才能进入 `shift_bind`，不能由 schedule
按名字反推或强塞成员。

| 绑定 ID | 干员 | 规则 | 模块 |
|---------|------|------|------|
| `rosemary_blackkey` | 迷迭香、黑键 | 同队；αβγ 周期内上岗 2 班、休息 1 班 | `schedule/shift_bind.rs` |
| `human_fireworks_*` | 乌有、实际入选的重岳/令；纯分支另含桑葚 | 同队、上 2 休 1；桑葚休息班办公室合法补位 | `AssignmentPlan` + `team_rotation.rs` |
| `producer:*` | 规则实际 producer + 实际贡献 consumer | `exact_presence` / `requires_presence` / `none`；仅有效贡献存在时派生 | `AssignmentPlan.resolved_producer_dependencies` |

八幡海铃绑定所有实际叙拉古贸易 consumer；戴菲恩绑定各房实际格拉斯哥 consumer；凛御银灰不生成自动 producer bind。不同贸易站不要求同房；涤火杰西卡关系为 `none`。未入选或未贡献者不绑定，schedule 不按名字或 tag 重推 producer。

**对齐**：若 peak 编制下绑定组成员落在不同 H1/H2 半区，`align_shift_binds_in_halves` 交换同类设施房间，使二者进入同一 cohort（α 或 β）。

**休息班次**（与队伍标签绑定）：

| 队 | 休息班 |
|----|--------|
| γ | S1（12h） |
| α | S2（6h） |
| β | S3（6h） |

单测：`team_rotation_rosemary_blackkey_shift_bind`。

---

## 6. CLI 与 MAA

```bash
cargo run -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --rotation 2 \
  --maa-out out/243_maa.json
```

- `--rotation`：可省略（默认 `3`），或取 `2`、`12/12/12`、`fiammetta-8844`、`abyssal-7575`
- stdout：账号画像摘要 + 对应 profile 的人类可读排班表 + 队伍花名册
- stderr：写出路径与运行元数据
- `--maa-out`：MAA 排班 JSON（见 `export/maa.rs`）

仅排班、不需要账号画像时：

```bash
cargo run -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --rotation abyssal-7575 \
  --maa-out out/243_maa.json
```

不存在 A-B-A 兼容入口；旧调用应直接迁移到 `plan` 或 `layout team-rotation`。

---

## 7. 与编排层的关系

- **单班编制**（peak/recovery）：`assign_shift` → 编排 `System → Plan → Execute`（见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)）。
- **多班轮换**：`AssignmentPlan` 只声明或从 peak 实际落位派生 `shift_bind`；schedule
  在 peak 编制之上切半 + γ 替补，并唯一负责执行 αβγ 归队与同上同下。
- 宿舍成员：由 `pinned_assignment_excluding` 从 peak 拷贝并共享。
- 办公室成员：普通办公室岗位不进入 shared 或 αβγ cohort；每班生产和固定岗位完成后，
  从当班剩余候选按办公室效率和当前班 ETA 独立选择。真实绑定成员随其消费方体系同上同下，
  候选耗尽不静默扩大池子。
- 中枢：不在 `pinned_assignment` 中钉死；每班按活跃两队重搜/补位，休息队不得上岗。
- 迷迭香感知 producer：不作为完整 `rosemary_perception` System 进编；由 global effect + `assign_perception_producers` 处理（Phase 4）。
- 黑键贸易站：`blackkey_closure` 仍是 L3 shortcut / segment 锚点；排班入口由 `closure` role 选择，不再作为 fixed registry 早占站。

`TeamRotationReport.peak_plan` 携带完整 `AssignmentPlan`（JSON 可序列化）；text 输出打印已选体系与贸易 meta 房间。

### 7.1 peak 主力最长工作时间

`schedule_team_rotation` 在当前启发式流程选出的 peak assignment 生成后立即调用 mood ETA 内核，
把以下信息写入 `TeamRotationReport.peak_mood_eta`：

- 每名在岗生产/功能干员的净心情消耗；
- 每名干员从满心情到休息阈值的可工作时间；
- 首个瓶颈干员；
- peak 主力班的最长工作时间。

CLI 文本和 CSV 会展示该锚点，JSON 保留完整 `per_op` 明细。默认 ABC 仍固定
`12h + 6h + 6h`，不会根据 ETA 自动改型；显式 2 / 4 班按 profile 固定时长检查完整循环中的连续工作段和 `0.5h` 余量。

### 7.2 菲亚梅塔主力回岗覆盖

菲亚梅塔次数、固定班次、目标去重、连续工作和布局优先级统一见
[菲亚梅塔使用规则](菲亚梅塔使用规则.md)。本节只保留接口事实：回岗覆盖默认启用，
可用 `--no-fiammetta` 或 `options.fiammetta_enable=false` 关闭；MAA 只序列化排班层
已经生成的 `TeamShiftResult.fiammetta`，不自行选人或补动作。

---

## 8. 相关文件

| 文件 | 作用 |
|------|------|
| `schedule/team_rotation.rs` | ABC 主流程 |
| `schedule/shift_bind.rs` | 班次绑定定义与对齐 |
| `layout/assign.rs` | `pinned_assignment_excluding`、`assign_control`、`assign_team_gamma_half`（γ 贸易 role + plain） |
| `search/role_pick.rs` | `docus` / `closure` / `witch` / `witch_fallback` 贸易 role fallback 链 |
| `search/control.rs` | 中枢候选搜索、`ControlInjectRawSumV0` 排序、补位策略 |
| `schedule/base_rotation.rs` | `evaluate_base_assignment_efficiencies`（ABC 的逐房直接效率结算） |
| `infra-cli/commands/layout.rs` | `team-rotation` 子命令 |
| `export/maa.rs` | MAA JSON 导出 |

---

## 9. Agent 提示

- **跑模拟** → 默认 `plan`，需要纯排班时才用 `layout team-rotation`；`layout test` 只做单班探测。
- **改普通二班中枢** → `schedule/team_rotation.rs` 的 `main_backup_12_12` 状态构造 + 中枢独立求解入口；不要复用 ABC 的 `build_team_control_map` 或生产切半结果。
- **改 ABC 中枢轮休 / 补位** → `schedule/team_rotation.rs` + `layout/assign.rs` + `search/control.rs`；不要把中枢重新放回 `pinned_assignment`。
- **改迷迭香/黑键同休** → `shift_bind.rs` + `team_rotation.rs`。
- **改但书 / 可露希尔 / 龙巫 meta 取舍** → `trade_segments.json` 的 `roles` + `search/role_pick.rs`；不要把 core priority 写回 fixed registry。
- **改 peak 编制** → 编排层 / `assign_shift`，见 [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)。

γ 半班生产搜索使用实际共存上下文：H1 合并 β 与当前 γ partial，H2 合并 α 与当前 γ partial；每个贸易/制造房落位前重新 resolve。中枢 seed 按实际 `shift_bind` 的生产半区过滤：正在被 γ 接替的半区处于休息态，其绑定 producer 不进入候选上下文；另一活跃半区的绑定 producer 保留。不能复用静态 peak `production_layout` 或 peak 全中枢代替当班共存成员。

未形成 peak bind 的可选动态 producer 不会被轮换层强塞，只在普通中枢搜索实际选中时按每班实际在场结算收益；γ 班允许普通搜索新增未绑定的目标标签 consumer，其收益只按该班最终实际在场状态结算，不要求轮换层后置新建 cohort 或反向绑定 producer。最终导出前统一校验每班中枢恰好 5 人、显式 bind 同上同下且满足上 2 休 1；不再按标签重推 producer/consumer presence。
