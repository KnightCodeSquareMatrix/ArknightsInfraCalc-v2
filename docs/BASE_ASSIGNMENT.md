# 全基建进驻编制（宏观排班）

> **状态**：设计定稿；**P0 已落地**（`assign_base_greedy`、`search_control_combos`、`filter_manufacture_pool`；`layout test` 默认调用宏观落位）。  
> 与贸易三班轮换（§8.10）并列：轮换只管**同一班次内 3 个贸易站**；本文管**单班全蓝图各房间谁上岗**。  
> 心情 / 宿管 / 跨班连班仍属非目标，见 [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) §8.12。

---

## 1. 解决什么问题

给定：

- `BaseBlueprint`（有几间贸易/制造/发电/中枢/宿舍等）
- `OperBox`（玩家拥有干员及练度）
- `operator_instances` + `skill_table`

输出一份 **`BaseAssignment`**：每个 `room_id` 进驻哪些干员，且**同一人只占一个岗位**。

当前 `layout test` / `bench` 只对贸易、制造**各搜一组最优三人**，不合并编制，**会出现同一人重复出现在两份结果里**。宏观排班要在输出前用全局 `used` 集合消歧。

---

## 2. 设计原则（已定，不再讨论）

| 原则 | 说明 |
|------|------|
| **设施内并行** | 贸易 / 制造 / 发电 / 中枢（及日后宿舍 producer）搜索可 rayon 并行；不为软搭档（如 Christine 无酒神）做组合剪枝 |
| **规模可控** | 单设施 `C(n,3)` 或发电 `O(n)`；瓶颈在单次 `solve_*` 热路径，不在穷举次数 |
| **硬约束合并** | 一人不能占两位：并行搜完后**顺序落位** + `HashSet<name>`，不是为省算力的剪枝 |
| **不做全局联合最优** | 禁止 `C(n,3)^站数` 式笛卡尔积；贪心 + `top_k` 回退即可 |
| **机制在 core** | 编制编排可放 `infra-core::layout`；CLI 只加载 JSON、调用、打印 |

---

## 3. 两阶段流水线

```
operbox + blueprint
        ↓
┌───────────────────────────────────────────────────┐
│ 阶段 A：分设施搜索（可并行）                        │
│  · 中枢  C(n,k)  k∈[1,5]（待实现 search_control） │
│  · 贸易  每站 C(n,3)；meta 站带 shortcut 过滤       │
│  · 制造  每产线类型 C(n,3)（同 243c split-line）   │
│  · 发电  每站 1 人贪心（已有 search_power_*）      │
│  · 宿舍   producer 房（森西等）按 assignment 定人   │
│  每站缓存 top_k 候选（默认 k≥20 可调）              │
└───────────────────────────────────────────────────┘
        ↓
┌───────────────────────────────────────────────────┐
│ 阶段 B：顺序落位（单线程，极快）                    │
│  global used: HashSet<String>                       │
│  按 §4 默认顺序，逐房间认领干员                     │
│  冲突 → 取该站 top 下一项，或对 filter 后池重搜     │
└───────────────────────────────────────────────────┘
        ↓
   BaseAssignment
        ↓
   resolve_base()  →  TradeLayoutContext + 各房局部 layout
        ↓
   （可选）对 consumer 房再跑单房 solve 出分 / bench 输出
```

**`resolve` 与搜索的次序**：凡影响 `layout.global` / `global_inject` 的 **producer 房**（中枢、宿舍森西、发电虚拟电站等）须先落位并 `resolve_base` 一轮，再搜依赖全局资源的 **consumer 房**（贸易齐尔查克、制造自动化等）。实现上可以是「producer 阶段落位 → resolve → consumer 阶段并行搜+落位」，不必一次 resolve 定死。

---

## 4. 默认落位顺序

优先级从高到低（前者先占 `used`）：

1. **控制中枢** `control`（木天蓼 / 全局贸易·制造 % 等）
2. **宿舍 producer**（如森西 → 魔物料理；令/夕等烟火链日后同列）
3. **贸易 meta 站**（但书 → 龙巫 → 可露希尔；逻辑同 `schedule/trade_rotation.rs` 的 `hit_filter`）
4. **制造各产线**（按蓝图 `manu_line_scenario`：赤金线、经验线等同组三人）
5. **发电各站**（每站 1 人，`search_power_assignment` 同款 `used`）
6. **贸易余站**（赤金 / 源石普通三人组）

同类型多房间：按蓝图 `rooms` 数组顺序或稳定 `room_id` 字典序。

---

## 5. 冲突处理（默认）

对当前房间：

1. 从阶段 A 缓存的 `top_k` 中，取**第一个**与 `used` 无交集的组合；
2. 若 `top_k` 耗尽，对该房用 `filter_*_pool(pool, &used)` **缩小池后重搜**（仍 `C(n',3)`，`n'` 已减小）；
3. 认领成功后，将组合内所有 `name` 插入 `used`。

贸易站已有先例：`pool/trade.rs` 的 `filter_trade_pool` + `schedule/trade_rotation.rs` 的 `commit_station`。

发电站已在 `search/power.rs` 内对多站使用 `used`。

**不为**「Christine 必须同房酒神」等软条件做预剪枝；无搭档时机制给 0 分，靠排序自然淘汰。

---

## 6. 各设施搜索约定

| 设施 | 每房人数 | 搜索入口（现有） | 编制备注 |
|------|----------|------------------|----------|
| 控制中枢 | 1～5 | `solve_control`（**无** search 包装，待补） | `control_operators()` |
| 贸易站 | 3 | `search_trade_triples` / `search_trade_triples_filtered` | 同房互斥：`trade_station_exclusive_violation` |
| 制造站 | 3 | `search_manufacture_triples` | 同类产线共用三人（`ManuSearchRecipeMode::Lines`） |
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

| 命令 | 现状 | 目标 |
|------|------|------|
| `layout test` | `BaseAssignment::default()`，贸易/制造分搜，**可重复上岗** | 调用宏观落位 → 输出 `BaseAssignment` + 各房分数 |
| `bench` | 同上，243c 固定蓝图 | 可选接入编制 |
| `schedule rotation` | 仅贸易 3 站 × 3 班 A-B-A，`used` 仅在**单班 9 人**内 | 不变；全基建编制是**单班快照** |
| `search trade` | 单设施探索 | 不变 |

---

## 9. 非目标（实现时勿扩 scope）

- 心情预算、宿管恢复、8h 上班表
- 制造 / 发电 / 中枢的**三班轮换**
- 全局最优（整数规划 / 模拟退火）
- 软搭档组合剪枝（无效组合靠低分淘汰）
- 在 CLI 内写机制公式

---

## 10. 实现清单（待开发）

| 项 | 模块建议 | 说明 |
|----|----------|------|
| `assign_base_greedy` | `layout/assign.rs` 或 `layout/resolve.rs` | §3～§5 主入口 |
| `search_control_combos` | `search/control.rs` | 中枢 `C(n,k)` + rayon |
| `filter_manufacture_pool` | `pool/manufacture.rs` | 对称 `filter_trade_pool` |
| `layout test --assignment` | `infra-cli/commands/layout.rs` | 读可选 JSON，写出编制 |
| 测试 | `layout/resolve.rs` | 全图无重名；森西宿舍 → 齐尔查克贸易分 |

---

## 11. 代码索引

| 文件 | 职责 |
|------|------|
| `layout/assignment.rs` | `BaseAssignment`、`AssignedOperator` |
| `layout/blueprint.rs` | `BaseBlueprint`、产线/贸易站场景推导 |
| `layout/resolve.rs` | `resolve_base`、全局资源 producer 注入 |
| `layout/workforce.rs` | `WorkforceIndex`、`apply_to_layout` |
| `schedule/trade_rotation.rs` | 贸易班内 `used` + meta 站贪心（**参考实现**） |
| `search/trade.rs` / `manufacture.rs` / `power.rs` | 分设施搜索 |
| `pool/trade.rs` | `filter_trade_pool` |

---

## 12. 规模直觉（n≈60，243c）

| 阶段 | 求值次数（量级） |
|------|------------------|
| 阶段 A 并行四设施 | 各设施独立：贸易 ~10⁵、制造 ~10⁵、发电 ~10²、中枢 ~10⁶（若穷举 5 人） |
| 阶段 B 落位 | `O(房间数 × top_k)`，可忽略 |
| 合并后 | 单次 `solve` 保持微秒～毫秒级时，总耗时仍为 **秒级** |

设施级并行 + 设施内 rayon 即可；**不必**为宏观排班再做算法层「优化」。
