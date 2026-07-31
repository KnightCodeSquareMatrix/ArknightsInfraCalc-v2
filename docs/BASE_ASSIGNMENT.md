# 全基建进驻编制（宏观排班）

> 文档角色：canonical
> 生命周期状态：current
> 领域键：layout.assignment
> 当前真源：self
> 摘要：裁决全基建单班进驻编制规则

> **状态**：**已落地**（`assign_base_greedy`、`assign_shift`、`layout/orchestrate::{build_plan, execute_plan}`、`search_control_combos`、`assign_dorm_producers`、`assign_manufacture_lines`、`assign_power_stations`、`assign_trade_remainder`；`layout test` 默认调用宏观落位）。
> 多班轮换见 **[SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md)**（αβγ ABC 唯一现行路径；A-B-A 已移除）。本文管**单班全蓝图各房间谁上岗**。
> 心情 / 宿管 / 跨班连班仍属非目标，见 [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) §8.12。

---

## 1. 解决什么问题

给定：

- `BaseBlueprint`（有几间贸易/制造/发电/中枢/宿舍等）
- `OperBox`（玩家拥有干员及练度）
- `operator_instances` + `skill_table`

输出一份 **`BaseAssignment`**：每个 `room_id` 进驻哪些干员，且**同一人只占一个岗位**。

当前 `layout test` **默认已调用 `assign_base_greedy`** 进行全基建宏观排班，用全局 `used` 集合消歧，同一人不会重复上岗。`bench` 仍为固定 243 基准分搜模式（仅用于搜索基准对比）。

---

## 2. 设计原则（已定，不再讨论）

| 原则 | 说明 |
|------|------|
| **单房候选并行** | 单个贸易 / 制造房的候选组合求值、单个发电站的候选求值可用 rayon；这不表示多个制造房共享同一份静态上下文并行搜索 |
| **规模可控** | 单设施 `C(n,3)` 或发电 `O(n)`；瓶颈在单次 `solve_*` 热路径，不在穷举次数 |
| **跨房顺序提交** | 一人不能占两位，且已上岗 workforce 会影响后房效率：制造房按稳定顺序执行「合并已提交 assignment → `resolve_base` → 搜本房 → commit + 更新 `used`」 |
| **不做全局联合最优** | 禁止 `C(n,3)^站数` 式笛卡尔积；贪心 + `top_k` 回退即可 |
| **机制在 core** | 编制编排可放 `infra-core::layout`；CLI 只加载 JSON、调用、打印 |

---

## 3. 两阶段流水线

```
operbox + blueprint
        ↓
┌───────────────────────────────────────────────────┐
│ 阶段 A：编排 + producer 落位                        │
│  · System → Plan → Execute 先认领 registry 体系     │
│  · 中枢与宿舍 / 感知 producer 先落位                │
│  · resolve_base 得到当前全局资源                    │
└───────────────────────────────────────────────────┘
        ↓
┌───────────────────────────────────────────────────┐
│ 阶段 B：按设施生命周期搜索并提交                     │
│  · global used: HashSet<String>                     │
│  · 贸易 / 发电按各自 fill 顺序搜索、提交             │
│  · 制造按稳定房序逐房：                              │
│      合并已提交 assignment → resolve_base            │
│      → 本房 C(n,k)（组合内部可 rayon）→ commit       │
│  · 冲突时过滤 used 后重搜；不做跨房笛卡尔积           │
└───────────────────────────────────────────────────┘
        ↓
   BaseAssignment
        ↓
   resolve_base()  →  LayoutContext + 各房局部 layout
        ↓
   （可选）对 consumer 房再跑单房 solve 出分 / bench 输出
```

**`resolve` 与搜索的次序**：凡影响 `layout.global` / `global_inject` 的 **producer 房**（中枢、宿舍森西、发电虚拟电站等）须先落位并 `resolve_base`，再搜依赖全局资源的 **consumer 房**（贸易齐尔查克、制造自动化等）。制造还存在跨制造房的 workforce 依赖，因此不能把所有制造房先用同一快照并行搜完再落位；每提交一房，下一房都基于合并后的当前编制重新 `resolve_base`。这里仍然是逐房贪心，不做房间组合的全局笛卡尔积。

---

## 4. 默认落位顺序

优先级从高到低（前者先占 `used`）：

1. **registry 体系**（`base_systems.json` → `build_plan` → `execute_plan`；跨站优先，同站次之）
2. **控制中枢补位** `control`（木天蓼 / 全局贸易·制造 % 等；不足 5 人时用 `search_control_combos` 补满）
3. **宿舍 / 感知 producer**（如森西、爱丽丝/车尔尼的梦境/小节→感知源；塑心作为迷迭香/黑键链的宿舍无声共鸣可选插件；先落位再 `resolve_base`）
4. **发电各站**（每站 1 人，`search_power_assignment` 同款 `used`）
5. **贸易余站**（多贸龙门币默认核心顺序为 `docus → closure → witch`，再进入 `witch_fallback → karlan → penguin → plain`；格拉斯哥为普通候选，源石单走 plain）
6. **制造各产线**（按蓝图 `manu_line_scenario`：各制造房按配方独立搜索 `C(n,k)`）

同类型多房间：按蓝图 `rooms` 数组顺序或稳定 `room_id` 字典序。

**贸易 core priority**：只看蓝图中的实际贸易站数量、等级和订单。恰有 1 间贸易站且为龙门币订单时，可露希尔是 required core；至少 2 间贸易站且任一为龙门币订单时，但书是首个 required core。多贸龙门币默认顺序为但书 > 可露希尔 > 完整龙巫：但书优先进入可用的最低等级金单站，可露希尔随后优先进入可用的最高等级金单站，完整龙巫只进入能容纳巫恋 + 龙舌兰 + 合法裁缝 β/α 的三级金单站。全部贸易站均为源石订单时这些金单核心都不上，混合订单中的源石站也不被金单核心占用。双贸易站中 Rosemary/黑键体系 alternative 确认激活，且可露希尔与完整龙巫都可形成时，Plan 进一步把可露希尔+黑键固定为 B，并把完整龙巫 C 留给 γ 替补；不能先按当前房间结果反推体系是否激活。A/B 队友仍由各房正式 `final_efficiency` 搜索决定。八幡海铃、伺夜、贝洛内都不是编排硬核心，也不通过 registry 强制进编：伺夜、贝洛内可上 0/1/2 人且不预设同房，full-E2 下 A 期望自然选中伺夜+贝洛内。`gsl_docus_solo` / `gsl_docus_syracusa` 只在求解实际同房候选时提供机制结算，不参与总 core 顺序或队友候选优先顺序。缺可露或缺完整龙巫时分别沿后续 role 降级，不用巫恋+龙舌兰二人组伪造自动龙巫；普通白板组合只保留显式单站结算兼容，不进入自动 role。

**动态 producer 当前边界**：八幡海铃、戴菲恩、凛御银灰和涤火杰西卡由一次 producer presence 子集枚举比较完整生产前缀；`vina_lungmen` / `meta_vina` 不再参与选型，灵知精密计算也不替代凛御银灰商业版图。贸易/制造多房内部仍是上述有序填房 policy；完整条件响应行 join 与 Bake/live 等价证明见 [CONTROL_CENTER_ASSIGNMENT.md](CONTROL_CENTER_ASSIGNMENT.md) 和 [DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

联合候选的目标 policy 已裁决为贸易、制造分域 `final_efficiency` 总和按显式权重比较，当前
默认 `1:1`；未来可调整该 policy 权重，布局本身继续负责宏观产出结构。该 comparator 已在
shadow 候选路径实现，公开默认仍使用旧 comparator；exact joint 尚未完成，当前结果仍只在
有序填房 policy 限制空间内解释。

---

## 5. 冲突处理（默认）

对当前房间：

1. 贸易等持有候选报告的 fill 可从当前报告选择与 `used` 无交集的组合，耗尽后对当前房过滤 `used` 并重搜；
2. 排班制造不消费阶段 A 缓存的 `top_k`：每个制造房都先过滤当前 `used`，合并已提交 assignment 并重新 `resolve_base`，再当场执行本房 `C(n',k)`；
3. 当前房命中后立即 commit，并将成员写入 `used`；下一制造房因此看到最新 workforce 与占用状态。

贸易站落位实现见 `layout/assign/commit.rs` 与 `layout/assign/trade_fill.rs`。

发电站已在 `search/power.rs` 内对多站使用 `used`。

**不为**「Christine 必须同房酒神」等软条件做预剪枝；无搭档时机制给 0 分，靠排序自然淘汰。

---

## 6. 各设施搜索约定

| 设施 | 每房人数 | 搜索入口（现有） | 编制备注 |
|------|----------|------------------|----------|
| 控制中枢 | 1～5 | `search_control_combos` | `ControlFillPolicy::InjectOnly` / `LayeredFill`；输出具名 policy key，不输出生产效率 |
| 贸易站 | 按等级 1/2/3 | `search_trade_triples` / `search_trade_triples_filtered` | 同房互斥：`trade_station_exclusive_violation` |
| 制造站 | 按等级 1/2/3 | `search_manufacture_triples` | 排班对每房使用 `ManuSearchRecipeMode::Single(recipe)` 独立搜索；`Lines` 仅用于 bench / 产线探索，不是排班多房共享组合 |
| 发电站 | 1 | `search_power_assignment` | 站内不重复 |
| 宿舍 | 1～N | 暂无 search；编制驱动 `resolve` 内 producer | 森西大食堂等 |

243c 典型岗位数（单班）：中枢 ≤5 + 贸易 9 + 制造 12 + 发电 3 ≈ **29 人·位**（同一人仅占一位）。

---

## 7. 输出：`BaseAssignment`

类型：`infra-core::layout::assignment::BaseAssignment`（已实现 JSON serde）。

```json
{
  "rooms": [
    { "room_id": "control", "operators": [
      { "name": "火龙S黑角", "elite": 2 },
      { "name": "麒麟R夜刀", "elite": 2 }
    ]},
    { "room_id": "trade_1", "operators": [
      { "name": "巫恋", "elite": 2 },
      { "name": "龙舌兰", "elite": 2 },
      { "name": "空弦", "elite": 2 }
    ]}
  ],
  "training_assist": null,
  "base_workforce": []
}
```

| 字段 | 含义 |
|------|------|
| `rooms[].room_id` | 与蓝图 `rooms[].id` 一致 |
| `operators[].elite` | `0` 精0，`1` 精1，`2` 精2（`PromotionTier`） |
| `training_assist` | 训练室副手（计入 workforce 规则时不进杜林等计数） |
| `base_workforce` | 仅进驻名单、不占具体房间时（如杜林族计数）；覆盖 `scenario.base_workforce` |

`resolve_base(blueprint, assignment, …)` 已消费该结构；缺省房间视为空岗。

---

## 8. 与现有命令的关系

| 命令 | 当前用途 | 备注 |
|------|----------|------|
| `layout test` | 默认调用 `assign_base_greedy`；传 `--assignment` 时消费用户给定编制 | 自定义布局 + operbox 的单班搜索 / 效率探测入口 |
| `bench` | 固定 243 基准分搜 | 不代表宏观编制；不要用它替代 `layout test` |
| `plan` | 账号画像 + 定时换班 profile + 可选 MAA 导出 | 用户说“跑一遍模拟”时的推荐入口；默认 ABC，可显式 2 / 具名 4 班 |
| `layout team-rotation` | 默认 ABC、二班主替、菲亚 / 深海具名四班 | 当前全基建多班轮换入口 |
| `search trade` | 单贸易站探索 | 不做制造 / 全基建编制 |

---

## 9. 非目标（实现时勿扩 scope）

- 心情预算、宿管恢复、全基建连续时间最优化
- 全局最优（整数规划 / 模拟退火）
- 软搭档组合剪枝（无效组合靠低分淘汰）
- 在 CLI 内写机制公式

---

## 10. 实现状态

| 项 | 位置 | 状态 |
|----|------|------|
| `assign_base_greedy` / `assign_shift` | `layout/assign.rs` | ✅ 已落地 |
| `search_control_combos` | `search/control.rs` | ✅ 已落地（含 `ControlFillPolicy`） |
| `filter_manufacture_pool` | `pool/manufacture.rs` | ✅ 已落地（基于 `filter_pool` 泛型） |
| `assign_dorm_producers` | `layout/assign.rs` | ✅ 已落地（森西宿舍 producer） |
| `layout/orchestrate::{build_plan, execute_plan}` | `layout/orchestrate/` | ✅ 当前主路径（`base_systems.json` 认领；tier 两阶段：`CrossStation` → `SameStation`） |
| `claim_base_systems` | `layout/system.rs` | ✅ 兼容 / 测试辅助 API；主路径已迁到 `orchestrate` |
| `layout test --assignment` | `infra-cli/commands/layout.rs` | ✅ 已落地（默认调用 `assign_base_greedy`） |
| 单元测试 | `layout/assign.rs` tests | ✅ 无重名、黑键≠巫恋、金线 trio、怪猎中枢 |
| MAA 导出 | `export/maa.rs` | ✅ `layout team-rotation --maa-out` |
| **待增强** | `layout/assign.rs` / `schedule/` | αβγ 三队轮换需更多回归测试；制造 bond/anchor 策略等待公孙夹具 |

---

## 11. 代码索引

| 文件 | 职责 |
|------|------|
| `layout/assignment.rs` | `BaseAssignment`、`AssignedOperator` |
| `layout/blueprint.rs` | `BaseBlueprint`、产线/贸易站场景推导 |
| **`layout/assign.rs`** | **`assign_base_greedy` / `assign_shift`：宏观排班主入口** |
| `layout/orchestrate/` | `build_plan` / `execute_plan`：System 选型与 fixed/bond/pick_one 落位 |
| `layout/system.rs` | `base_systems.json` 解析、registry claim 兼容 API |
| `layout/resolve.rs` | `resolve_base`、全局资源 producer 注入 |
| `layout/workforce.rs` | `WorkforceIndex`、`apply_to_layout` |
| `schedule/shift_bind.rs` | 迷迭香+黑键等同上同下约束 |
| `schedule/base_rotation.rs` | `evaluate_base_assignment_efficiencies`：ABC 逐房直接效率结算 |
| `schedule/team_rotation.rs` | `schedule_team_rotation`：αβγ 三队轮换 |
| `search/control.rs` | `search_control_combos`：中枢 C(n,k) + `ControlFillPolicy` |
| `search/role_pick.rs` | 贸易 core role：`docus` / `closure` / `witch` / `witch_fallback` fallback 链 |
| `search/trade.rs` / `manufacture.rs` / `power.rs` | 分设施搜索 |
| `pool/trade.rs` | `filter_trade_pool` |
| `pool/base.rs` | 泛型 `PoolCore<T>`、`filter_pool` |

---

## 12. 规模直觉（n≈60，243c）

| 阶段 | 求值次数（量级） |
|------|------------------|
| 阶段 A 分设施求解 | 单房组合内部可用 rayon；制造房按顺序提交并逐房 resolve，不做房间笛卡尔积 |
| 阶段 B 落位 | `O(房间数 × top_k)`，可忽略 |
| 合并后 | 单次 `solve` 保持微秒～毫秒级时，总耗时仍为 **秒级** |

单房 `C(n,k)` 内 rayon 即可；制造房因跨房计数按顺序提交并逐房 resolve。**不必**为宏观排班构造房间笛卡尔积。

排班制造不由 standalone 工具表裁剪。多制造房按顺序提交，每房搜索前基于当前完整 assignment 重新 resolve；全部落位后再次按最终 assignment 结算并回写制造效率快照，避免早期房间保留陈旧跨房计数。
