# 评分口径审计（Scoring Model Audit）

> 状态：分量化口径版（2026-06-24）  
> 配套计划：[SCORING_REFACTOR_PLAN.md](SCORING_REFACTOR_PLAN.md)  
> 目标：记录项目中所有参与搜索、排序、排班汇总、CLI 展示的评分字段，明确其单位、用途和排序策略。当前结论是**不需要贸易-制造平衡公式**；跨域输出保留分量。

---

## 1. 口径分类

| 分类 | 单位 | 说明 |
|------|------|------|
| 贸易赤金订单效率 `%` | percent | 如 `order_eff_total` / `trade_pct`，含龙舌兰、可露希尔、但书等已折算等效效率 |
| 贸易赤金解释分量 `%` | percent | 如 `mechanic_equiv_eff_pct` / shortcut `gold_pct`，用于解释赤金需求或机制收益 |
| 贸易调试乘积 `multiplier` | multiplier | 如 `effective_eff_multiplier`，内部调试值，不替代 trade_pct / gold_pct |
| 制造赤金效率 `%` | percent | 赤金产线 `prod_total` |
| 制造经验效率 `%` | percent | 经验书产线 `prod_total` |
| 发电效率 `%` | percent | 如 `charge_speed_pct`，无人机充能速度 |
| 全局注入分量 `%` | percent | 中枢或跨设施向贸易/制造写入的效率注入，按 trade/manu_gold/manu_br 拆分 |
| 局部排序 key `%` | percent | 命名 policy 的排序键，例如 `ControlInjectRawSumV0` |
| 时长折算效率 | percent × hours/24 | 日排班汇总用，不改变原量纲 |

---

## 2. 总览表

| 模块 | 字段 / 函数 | 当前值来源 | 单位 | 当前用途 | 是否排序 | 后续处理 |
|------|-------------|------------|------|----------|----------|----------|
| trade search | `TradeSearchHit.score` | `result.order_eff_total` | 贸易赤金订单效率 % | 贸易三人组排序主键 | 是 | 保留或改名为 `sort_eff_pct` |
| trade search | `TradeSearchHit.trade_pct` | `result.order_eff_total` | 贸易赤金订单效率 % | 展示/解释 | 否 | 保留 |
| trade search | `TradeScoreBreakdown.effective_eff_multiplier` | `result.effective_eff_multiplier` | 调试乘积 | 内部排查单位产出/机制用 | 否 | 不作为用户侧最终倍率 |
| trade search | `unit_trade_per_day` | `result.production.unit.unit_trade_per_day` | 产量/天 | 展示 | 否 | 保留 |
| trade search | `mechanic_equiv_eff_pct` | `result.order_mechanic.mechanic_equiv_eff_pct` | 贸易赤金解释分量 % | 解释订单机制 | 否 | 与 `gold_pct` 同类解释，不参与跨域合成 |
| manufacture search | `ManuSearchHit.composite_score` | 单配方 `prod_total`；多线按线数加权 | 制造效率 % | 制造三人组排序主键 | 是 | 字段名后续可降歧义 |
| manufacture search | `ManuScoreBreakdown.prod_total` | `prod_base + prod_skill + prod_global` | 制造效率 % | 展示/解释 | 否 | 按赤金/经验配方解释 |
| power search | `PowerSearchHit.score` | `charge_speed_pct` | 发电效率 % | 发电站排序主键 | 是 | 保留 |
| power search | `virtual_power_equiv` | `virtual_power * VIRTUAL_POWER_MANU_EQUIV` | 临时解释值 | breakdown 展示 | 否 | 不参与当前排序；若要排序需新增 policy |
| control search | `ControlScoreBreakdown.total_score` | 普通：`current_control_inject_sort_score(...)` 维持 raw-sum；补位：`ancillary_score` | 局部排序 key % | 中枢组合排序 | 是 | 标注 policy，不称公式 |
| schedule scoring | `ShiftScores.trade_score` | sum of `effective_eff_multiplier` per trade room | 贸易调试/汇总值 | 排班评估/展示 | 否 | 用户侧优先看逐房 trade_pct / gold_pct |
| schedule scoring | `ShiftScores.manu_prod_sum` | sum of `prod_total` | 制造效率 % 汇总 | 排班评估/展示 | 否 | 保留 |
| schedule scoring | `ShiftScores.power_charge_sum` | sum of `charge_speed_pct` | 发电效率 % 汇总 | 排班评估/展示 | 否 | 保留 |
| schedule scoring | `weighted_*` | `score * shift_hours / 24` | 时长折算 | 日汇总展示 | 否 | 保留 |

---

## 3. 贸易搜索审计

文件：`crates/infra-core/src/search/trade.rs`

当前排序：

```rust
score: result.order_eff_total,
trade_pct: result.order_eff_total,
gold_pct: result.order_mechanic.mechanic_equiv_eff_pct,
```

`search_trade_single_order(...)` 中按 `trade_efficiency_sort_key(hit)` 排序。

当前含义：

- `score` 当前等于贸易赤金订单效率 `order_eff_total`；
- `trade_pct` 与 `score` 同值；
- 龙舌兰、可露希尔、但书等特殊机制由 L2/L3 或 shortcut 折算进贸易站效率；
- `effective_eff_multiplier` 存在于 breakdown，但当前不作为排序主键，也不作为用户侧结论；
- `unit_trade_per_day` / `output_multiplier` 用于调试产出，不参与当前排序；
- L3 shortcut 命中时，`trade_pct` / `gold_pct` 以公孙长乐等效效率锚点为准。

当前约定：

- 贸易 meta 复杂组合上不上班走 L3 shortcut / 编排认领；
- CLI 与文档应拆开展示贸易赤金订单效率与赤金解释分量；
- `effective_eff_multiplier` 仅保留为内部调试乘积，用于核对 solver / unit output。

---

## 4. 制造搜索审计

文件：`crates/infra-core/src/search/manufacture.rs`

当前排序：

- 单配方搜索按该配方 `prod_total` 排序；
- 多产线模式按赤金线数 / 经验线数对各自 `prod_total` 加权。

当前含义：

- 单配方：制造赤金或制造经验效率；
- 多产线：按产线数汇总当前布局下的制造效率；
- `storage` 仅作为 tie-break 或展示，不是主排序口径。

后续处理：

- 输出上尽量拆为 `manu_gold_eff` / `manu_battle_record_eff`；
- 字段名 `composite_score` 容易被误读为跨域综合分，后续可改名或在 JSON 文档中标注。

---

## 5. 发电搜索审计

文件：`crates/infra-core/src/search/power.rs`

当前排序：

```rust
pub fn power_station_score(charge_speed_pct: f64, _virtual_power_produced: f64) -> f64 {
    charge_speed_pct
}
```

当前含义：

- 发电站排序只看 `charge_speed_pct`；
- `virtual_power_produced` 当前不影响排序；
- `virtual_power_equiv` 在 breakdown 中保留解释，但不是当前排序依据。

后续处理：

- 如果虚拟发电需要参与某个局部排序，新增命名 policy；
- 不把虚拟发电塞进制造效率总分。

---

## 6. 中枢搜索审计

文件：`crates/infra-core/src/search/control.rs`

当前排序：

- `ControlFillPolicy::HrAndMood`：`total_score = ancillary_score`
- 默认 `Efficiency` 策略：
  - 分量：`trade_inject` / `manu_gold` / `manu_br`
  - policy：`ControlInjectRawSumV0`
  - sort key：`trade_inject + manu_gold + manu_br`

当前含义：

中枢普通排序仍使用历史 raw-sum，原因是这是局部补位选择，不是全局经济理论。该行为已通过 `scoring::current_control_inject_sort_score(...)` 标记为命名 policy。

风险：

`trade_inject`、`manu_gold`、`manu_br` 虽然都是 `%`，但不是一个需要长期宣称为“最终总分”的同质贡献。输出和文档必须保留分量，不能只给 raw-sum。

---

## 7. 排班层评分审计

文件：

- `crates/infra-core/src/schedule/base_rotation.rs`
- `crates/infra-core/src/schedule/team_rotation.rs`

当前评分：

- 贸易：`trade_score += result.effective_eff_multiplier`（历史内部调试/汇总字段；用户侧输出应优先展示逐房 `trade_pct` / `gold_pct`）
- 制造：`manu_prod_sum += result.prod_total`
- 发电：`power_charge_sum += charge_speed_pct`

自动排班生成的生产房间会在 `RoomAssignment.efficiency` 中保存求解阶段命中的效率快照：

- 贸易房保存 `trade_score` / `trade_pct` / `trade_skill_pct` / `trade_gold_pct`；
- 制造房保存 `manu_prod_total` / `manu_prod_skill` / `manu_storage_limit`；
- 发电房保存 `power_charge_speed_pct`。

`score_base_assignment` 优先读取该快照；只有手写或旧版 assignment 没有快照时才回退到 `resolve_base + solve_*` 重算。这样 CLI / 轮换展示不再成为第二套求解入口，也不会因为 assignment 只保存干员名导致 tier/progress 丢失。

`ShiftScores::weighted_*` 只做时长折算：

```rust
score * shift_hours / 24.0
```

当前含义：

排班层保留三类分量，不硬合成一个全局分。该设计应保留。

---

## 8. 后续审计 TODO

- [ ] 审计 `schedule/trade_rotation.rs` 的 `total_score` 是否只是 trade hit score 求和；
- [ ] 审计 `layout/assign.rs` 中各设施落位是否使用了正确排序字段；
- [ ] 审计 `box_profile/eval.rs` 是否混用了产量和效率；
- [ ] 审计 CLI 输出字段命名是否会误导用户；
- [ ] 确认 `data/trade_shortcuts.json` 中 shortcut 的效率口径；
- [ ] 继续补公孙等效效率锚点和实例。
