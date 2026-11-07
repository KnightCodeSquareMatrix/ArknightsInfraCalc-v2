# 心情消耗 / ETA / 宿舍回复建模

> 文档角色：archive
> 生命周期状态：historical
> 替代项：docs/排班/排班模式.md；docs/排班/定时换班.md
> 历史原因：心情与 ETA 分阶段设计快照，未完成能力不作为当前实现说明
> 快照日期：2026-07-18
> 摘要：保存心情消耗、ETA 和宿舍回复的历史设计

> 目标：给每个排班班次算出 ETA（干员心情耗尽前的最长工作时间），并把宿舍回复建模进去，让 αβγ 轮换的时间分配可以从固定 12/6/6 切换到按 ETA 推导。

> 当前状态（2026-07-11）：P1 心情内核已完成；P2 已先接入 peak 主力最长工作时间
> 锚点（`TeamRotationReport.peak_mood_eta`），固定 12/6/6 暂未改变；模板选优与 P3–P4 尚未实现。
> 依据：公孙长乐《心情消耗回复和工休时间.docx》（已存入 memory `mood-model-spec.md`）。

## 背景 / 现状

- 全项目 mood 硬编码为 `24.0`（满心情）：`AssignBaseOptions.mood = 24.0`、每个 `resolve_base` / trade / manu / power solver 调用都传 24。**从不衰减**。
- 班次时长是常量：`team_rotation.rs` 里 `12.0 / 6.0 / 6.0`；MAA 导出里 `12.0`。**没有 ETA 概念**。
- effect 模型里已经声明了 `Action::MoodDrainDelta` / `Action::MoodDrainPerStateStep` / `MoodDrainScope` / `Selector::Mood` / `Condition::MoodAbove/BelowOrEq`，但**没有任何 interpreter 消费 drain 动作**（`Selector::Mood`/`Condition::Mood*` 只被少数阈值技能读，且读的是硬编码的 24）。属于半成品脚手架。
- `MECHANICS_REGISTRY.csv`（727 行）里所有 `心情每小时消耗±X` / `恢复+X` 只在自由文本 `游戏原文` 列，未结构化，未被 Rust 加载。
- 心情**不影响生产速率**（阿克游戏机制：满/非满心情产率相同，红脸才掉到 1%/5%）。所以本功能只做 ETA 与回复，不动现有产出评分。

## 模型（来自 docx，确定性公式）

### 工作消耗

- 心情上限 24，基础工作消耗 **1.0/h**。
- 中枢满 5 人 → 全局 **−0.25**（→0.75）。
- 贸易/制造的设施减免（docx 的 X）按**当前进驻人数**：1 人无、2 人 −0.05、3 人 −0.1；发电/办公/会客/专精无。满编时通常与 L1/L2/L3 对应，但半空房必须按实际人数算。
- 干员自身心情技能（Y 的一部分）：算术叠加，如斥罪 +0.5、泡泡 −0.25。
- 干员向设施提供的减免（黍 −0.1、火哨 −0.1、巫恋 +0.25）：作用于同设施全员，可叠加。
- 中枢全局回复类（玛恩纳 / 维什戴尔 / 重岳）的直接效果**取最高不叠加**；笑脸类每名提供者 +0.05、彼此叠加，玛恩纳把其总和扩散全局。
- 槐琥/令"消除自身心情消耗影响"：抵消**干员自身**技能的 mood delta（正负都消），不影响设施/中枢来源。
- 净消耗 `x = max(0, 1.0 + Σdelta)`；若 `x <= 0` 则该干员在岗即"休息中"，ETA 无限。
- 单干员可工作时长 `= (mood_cap - rest_threshold) / x`（默认阈值 0，即 `24/x`）。

### 宿舍回复
- 白字固定回复：`1.5 + 0.1 × 宿舍等级`（L1→1.6? docx 表给的是 L1=2, L2=2.5, L3=3, L4=3.5, L5=4 —— 按表为准，即 `1.5 + 0.5×level`… 实际 `1.5+0.1×氛围`；**以 docx 表 5 个离散值为准，直接查表**）。
- 干员回复技能：自回 / 单回 / 群回 / 定向，四类；**同类取最高，不同类叠加**。
- 菲亚梅塔：自身 +2 且不吃外部；建模为数值 2.0 的单体回复。
- 冰酿：总额 0.8 平摊（4 人→0.2/人群回、2 人→0.4/人、1 人→0.8 单回）。
- 单回快照/进驻顺序：**精确模拟**。`RoomAssignment.operators` 是有序 `Vec`，其顺序即游戏进驻顺序。规则：
  - 单回宿管排在其受益者之前 → 锁定它之后**第一个**进驻的室友。
  - 单回排在后面 → 从**已进驻**的室友里挑当前心情最低的非宿管。
  - 心情最低这个 tiebreak 用 ETA 逐时模拟里的真实心情数值判定，因此是精确而非近似。

### ETA / 工休比
- 班次 ETA = 该班**所有在岗生产/功能干员**中最小的 `24/x`（跨设施组合由组内最快消耗者决定）。
- 工休比 `y/x`，最大工作占比 `y/(x+y)`，24h 最长工作 `24y/(x+y)`。

## 实现方案

### 1. 数据：`data/mood_model.json`（用户负责调参）
```jsonc
{
  "version": 1,
  "base_drain_per_hour": 1.0,
  "full_control_reduction": 0.25,     // 中枢满 5 人
  "facility_occupancy_reduction": {    // 贸易/制造，键为当前进驻人数
    "trade": {"1": 0.0, "2": 0.05, "3": 0.1},
    "factory": {"1": 0.0, "2": 0.05, "3": 0.1}
  },
  "dorm_recovery_by_level": {"1": 2.0, "2": 2.5, "3": 3.0, "4": 3.5, "5": 4.0},
  "rest_threshold": 0.0,               // 心情降到多少算必须下班
  "operator_mood_skills": [            // 从 CSV 解析后落到这里，用户可微调
    {"name": "斥罪", "facility": "office", "elite": 0, "self_drain_delta": 0.5},
    {"name": "泡泡", "facility": "factory", "elite": 0, "self_drain_delta": -0.25},
    {"name": "火哨", "facility": "trade", "elite": 0, "room_drain_delta": -0.1},
    {"name": "巫恋", "facility": "trade", "elite": 0, "room_drain_delta": 0.25}
    // ...
  ],
  "control_global_recovery": [         // 取最高，不叠加
    {"name": "玛恩纳", "elite": 2, "value": 0.1, "covers": ["power","office","meeting"], "spread_smiley": true}
  ],
  "mood_clear_self": ["槐琥", "令"],    // 消除同房自身 mood 影响
  "dorm_recovery_skills": [
    {"name": "菲亚梅塔", "kind": "single", "value": 2.0, "no_external": true},
    {"name": "斯卡蒂", "kind": "self", "value": 0.55}
    // ...
  ]
}
```
- 结构在 core 里定义 `MoodModel` + serde；`load()` 走现有 `data_path()`（含 embedded fallback，需在 `skill_table.rs::exact_embedded_data` 增一条 `mood_model.json`）。

### 2. 数据整理策略
- 这是一次性建模任务，直接从 `MECHANICS_REGISTRY.csv` 中检索心情相关原文并人工写入 `mood_model.json`，不保留一次性生成脚本。
- 无条件定值进入结构化条目；条件、计数、配对、定向和联动项保留原文进入 `todo`，等待实测或领域确认。

### 3. Core：新模块 `crates/infra-core/src/mood/`
- `mod.rs`：`MoodModel` 载入 + 查询 API。
- `drain.rs`：`fn operator_drain(name, elite, facility_kind, facility_level, room_ops, control_ops, model) -> f64`
  - 组装 base + 中枢满员 + 设施等级 + 自身技能（受 mood_clear_self 影响）+ 同房设施减免 + 中枢全局回复（取最高）。
- `recovery.rs`：`fn dorm_recovery(dorm_level, dorm_ops, model) -> HashMap<name, f64>`（按同类取最高 + 不同类叠加 + 菲亚/冰酿特例 + 单回按进驻顺序精确锁定受益者）。
- `eta.rs`：`fn shift_eta_hours(assignment, blueprint, model) -> ShiftEta { per_op: Vec<(name, drain, hours)>, bottleneck: (name, hours), eta_hours }`
- 纯函数、可单测；不依赖 solver，只读 assignment + blueprint + model。

## 两类用户 → 两种模式

排班有两类完全不同的用户，决定了 ETA 功能的两种用法：

- **模式一 · 定时手动换班**（学生/上班党，不能挂机，按固定钟点登录换班，如 16-4-4 / 12-6-6）。
  时长是**人为定死**的；mood 模型在这里当**校验器 + 选优器**：内置多套时间模板，逐一用心情/宿舍回复模型校验可持续性，选出当前 box 跑得动的最优模板。ETA 本身不驱动时长。
- **模式二 · MAA 驻守自动换班**（常驻按真实心情动态换班）。
  这是 ETA 的主场：目标是**最大化主力长班时长**（拉高主力工作时长占比 = 提高产出）+ **最优宿管分配**（群回/单回搭配、跨设施组合放回复最快宿舍），使循环可持续。mood 模型在这里当**联合优化器**用。

## 分阶段交付

联合优化是真正的组合优化问题，一次性全量 drop 风险高、难 review。分四个可独立交付、独立回归的 Phase；模式一在 **P2** 即上线，模式二在 **P4** 收口。

### P1 · mood 内核（地基）
- `data/mood_model.json` + core `mood/{mod,drain,recovery,eta}.rs` 纯函数。
- 用 docx 算例做黄金测试（斥罪 19.2h、泡泡 60h、满级宿舍 20.645h 等）。
- 交付物：给定一套 assignment + 宿舍布置，能算出每个干员净消耗 `x`、宿舍回复 `y`、单班 ETA、工休比。**独立可验证，不接任何上层。**

### P2 · 校验器 + 模式一多模板选优
- `AssignBaseOptions` 增 `shift_timing: ShiftTiming`：`Fixed`（现状 12/6/6，零回归默认）/ `Templates`（模式一）/ `Eta`（模式二，P4 才真正生效）。
- 内置时间模板集（16-4-4、12-6-6、18-6、12-12 等）。对每套模板：
  - 用 P1 的 mood 模型校验：每个主力干员在其工作时长内心情够不够（`工作时长 × x ≤ 24`）。
  - 校验休息队在休息段内能否回满足够心情（`需要 = 下段工作时长 × x`，`实际 = 休息时长 × y_dorm`）。
  - 宿舍容量约束（≤20 位、宿管占位）：休息队人数 + 宿管 > 可用位 → 不可行。
  - 可持续的模板里，按主力工作时长占比（产出）排序，选最优。
- `TeamShiftResult` 增 `mood_eta: Option<ShiftEta>` + `sustainable: bool`（`#[serde skip_if]`，不破坏现有 JSON）。
- 交付物：**定时换班用户可用**——给 box 选出能跑的最优固定时间模板 + 可持续性报告。

### P3 · 宿管分配求解
- 给定一套班次结构，把宿管（自回/单回/群回/定向，群优先、跨设施组合放回复最快宿舍）最优分配进宿舍，最大化休息队回复速率。
- 单回按进驻顺序精确锁定受益者（P1 已实现规则，这里做分配搜索）。
- 交付物：宿管布置建议，喂给 P2 校验和 P4 优化。

### P4 · 模式二联合优化
- 在 P2 校验器 + P3 宿管求解之上，搜索「长班时长 × 宿管布置 × 休息队轮换 × 菲亚等间隔插班」，输出**可持续的最长主力班 + 完整排班**。
- 迭代收敛：拉长主力班 → 校验休息队回复够不够（P2）→ 调宿管（P3）→ 再拉长，直到不可持续或到上限。
- 交付物：**MAA 驻守用户可用**——最大化产出的动态换班方案。

### CLI / 导出（贯穿各 Phase）
- `layout team-rotation` / `plan` 增 `--shift-timing fixed|templates|eta`（默认 fixed）。
- `main.rs` / `layout.rs` / `plan.rs` 组 `AssignBaseOptions` 处透传。
- MAA 导出 `duration_hours` 已读 `shift.duration_hours`，自动跟随。
- 文本/JSON 输出打 bottleneck 干员 + ETA + 可持续性 + 选中的模板/宿管布置。

### 测试（贯穿）
- `mood/drain.rs`：docx 算例黄金值——斥罪 1.25/h→19.2h、泡泡三级制造 0.4/h→60h、加玛恩纳+4笑脸 0.15/h→160h、巫恋组 +0.25、槐琥消除阿罗玛/火神。
- `mood/recovery.rs`：满级宿舍 4.0、一级 2.0；菲亚 2.0 单回覆盖 007 三人（0.65×3=1.95<2）；单回进驻顺序锁定。
- `mood/eta.rs`：满级宿舍三级制造 x=0.65 → 工休比 6.15、最大工作占比 86.02%、24h 最长 20.645h。
- P2：`Fixed` 模式仍精确 12/6/6（零回归）；`Templates` 对 243 fixture 选出满级宿舍 ~20/4 类模板。
- P4：`Eta` 模式主力班时长 > 固定 12h 且校验可持续。

## 范围边界

本版**做**（P1–P4）：
- 精确模拟宿舍进驻顺序快照（单回，利用 operators 有序 Vec）。
- 模式一多模板选优 + 模式二 ETA 驱动长班最大化 + 宿管联合优化 + 休息队可行性回溯。

本版**不做**（注明为下一步）：
- 不动生产评分（心情不改产率）。
- 菲亚以外的"完全离散自动化最优轮休"（docx 提到的理论极限，人类不可执行）——只做人类/定时可执行的批次轮休。
- 定向/联动回复（摩根、流明、塑心等）先按 CSV 能直给的建，联动项留 TODO 空着由用户后补。

## 涉及文件
| 文件 | 改动 |
|------|------|
| `data/mood_model.json` | 新增（人工整理 + 用户调参） |
| `crates/infra-core/src/mood/{mod,drain,recovery,eta}.rs` | 新增模块 |
| `crates/infra-core/src/lib.rs` | 挂 `mod mood` + re-export |
| `crates/infra-core/src/skill_table.rs` | embedded fallback 增 `mood_model.json` |
| `crates/infra-core/src/layout/assign.rs` | `AssignBaseOptions` 增 `shift_timing` |
| `crates/infra-core/src/schedule/team_rotation.rs` | ETA 驱动时长 + `TeamShiftResult.mood_eta` |
| `crates/infra-cli/src/commands/{layout,plan}.rs`、`main.rs`、`output.rs` | `--shift-timing` 透传 + 输出 |
| `docs/` | 新增 `MOOD_MODEL.md` 说明公式与近似 |
