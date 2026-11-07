# 评分口径收敛与分量化计划

> 文档角色：archive
> 生命周期状态：historical
> 替代项：docs/机制/EFFICIENCY_MODEL.md；docs/机制/SCORING_MODEL.md
> 历史原因：百分比兼容方案已被直接效率硬切替代，剩余开放项由 current owner 记录
> 快照日期：2026-07-18
> 摘要：保存评分口径收敛与分量化的历史实施计划

> 历史原状态：历史计划；百分比与兼容字段已被 2026-07-11 的直接效率硬切取代。当前实现见
> [EFFICIENCY_MODEL.md](../../机制/EFFICIENCY_MODEL.md) 与 [SCORING_MODEL.md](../../机制/SCORING_MODEL.md)。
> 已完成：Phase 0 首轮审计、Phase 1 注释修正、Phase 2 排序 key 显式化、Phase 3 `scoring` 分量策略入口、Phase 4 当前行为锁定测试；安全 warning hygiene 已完成。
> 当前结论：公孙长乐确认**不需要贸易-制造平衡公式**。龙舌兰、可露希尔、但书等特殊贸易机制已给出等效效率，直接进入贸易站赤金订单总效率；制造站继续拆为赤金效率、经验效率。
> 待做：把输出、文档和后续实现统一为分量化口径，不再等待或设计跨贸易/制造的单一复合分。

---

## 1. 背景

本项目曾预留过“贸易-制造平衡公式”入口，用来约束历史遗留的 `score` / `composite_score` / `total_score` 字段，避免 AI 自创权重。现在公孙长乐给出的新口径更直接：

- **贸易站**：龙舌兰、可露希尔、但书等机制已折算为贸易站赤金订单总效率；
- **制造站**：赤金产线效率、经验书产线效率分别作为制造分量；
- **跨域展示 / 比较**：保留分量，不强行合成一个全局综合分；
- **中枢注入排序**：当前仍可保留 `trade_inject + manu_gold + manu_br` 作为局部候选排序策略，但它只是 heuristic / policy，不是理论公式。

因此评分主线从“等待平衡公式”改为“分量口径清晰化 + 局部排序策略显式化”。

---

## 2. 总目标

### 2.1 必须达成

1. 所有参与排序的字段都明确单位：贸易赤金订单效率%、制造赤金效率%、制造经验效率%、充能效率%、局部排序 key 等。
2. 跨贸易/制造/全局注入不再伪装成单一理论分；需要排序时必须声明具体策略 ID。
3. 已有等效效率锚点进入各自设施分量，不再额外做贸易-制造平衡折算。
4. 当前排班结果尽量保持稳定，先改命名、文档和接口语义，再按锚点补数据。
5. 用测试锁定关键口径，防止后续又引入自创综合分。

### 2.2 非目标

- 不引入通用约束求解器。
- 不做全局 `trade * x + manu * y + power * z` 的任意综合分。
- 不重写 EffectAtom / L1-L3 求解语义。
- 不因为分量化调整而牺牲百毫秒级主路径性能。
- 不把中枢当前 raw-sum 排序当作理论锚点。

---

## 3. 核心原则

### 3.1 效率分量优先

| 口径 | 含义 | 例子 | 是否可直接排序 |
|------|------|------|----------------|
| 贸易赤金订单效率 | 贸易站赤金订单总效率，含龙舌兰/可露希尔/但书等已折算机制 | `order_eff_total`、shortcut `trade_pct` | 贸易站内可以 |
| 贸易赤金相关解释分量 | 赤金需求 / 订单机制等解释值 | `mechanic_equiv_eff_pct`、shortcut `gold_pct` | 展示/解释，不替代 trade_pct |
| 制造赤金效率 | 赤金产线制造效率 | `prod_total` with `RecipeKind::Gold` | 同制造赤金产线内可以 |
| 制造经验效率 | 经验书产线制造效率 | `prod_total` with `RecipeKind::BattleRecord` | 同制造经验产线内可以 |
| 发电效率 | 无人机充能速度 | `charge_speed_pct` | 发电站内可以 |
| 局部排序 key | 为某个局部选择策略生成的排序键 | `ControlInjectRawSumV0` | 仅限声明的局部策略 |
| 时长折算效率 | 日排班汇总用，`eff * hours / 24` | `weighted_*` | 用于日汇总/展示 |

### 3.2 站内搜索与跨域展示分层

| 层级 | 推荐口径 |
|------|----------|
| 贸易站 C(n,3) 搜索 | 贸易赤金订单效率 `trade_pct`；L3 shortcut 命中时使用公孙等效效率，`gold_pct` 单独展示 |
| 制造站 C(n,3) 搜索 | 按配方产线排序：赤金线看赤金效率，经验线看经验效率 |
| 发电站 O(n) 搜索 | 充能效率 `charge_speed_pct`；虚拟发电另列解释 |
| 控制中枢补位 | 输出 trade/manu_gold/manu_br 三个注入分量；当前 `Efficiency` 策略用 raw-sum 排序 |
| 体系选择 | 先走 `base_systems` priority / exclusive_group / fixture 锚点，不靠全局综合分发现体系 |
| 排班日汇总 | 保留 trade/manu/power 分量；不新增单一 balanced summary |

### 3.3 scoring 模块职责

`crates/infra-core/src/scoring/` 不再表示“平衡公式入口”，而是：

| 文件 | 职责 |
|------|------|
| `metric.rs` | `EffPct`、`ComponentScore` 等评分边界类型 |
| `components.rs` | 分量输入与命名排序策略，如 `ControlInjectRawSumV0` |
| `mod.rs` | 对外导出评分单位和策略入口 |

排序策略必须满足：

- 名称说明它只是 policy / heuristic；
- 不宣称为公孙公式；
- 不能作为最终理论锚点；
- 调用点附近保留原始分量字段，供 CLI / 前端解释。

---

## 4. 分阶段实施

### Phase 0：评分口径审计（已完成）

产出：[SCORING_MODEL.md](../../机制/SCORING_MODEL.md)

已列出所有 `score` / `composite_score` / `total_score` / `weighted_*` 字段，标记单位、用途、是否参与排序。

### Phase 1：修正文档和注释，不改行为（已完成）

已修正 trade / power / control 等注释中与实际排序口径不一致的部分。

### Phase 2：抽排序 key 函数，不改排序结果（已完成）

已完成：

- 贸易：`trade_efficiency_sort_key(hit)`；
- 制造：`manufacture_efficiency_sort_key(hit)`；
- 发电：`power_efficiency_sort_key(hit)`；
- 中枢：`control_inject_sort_key(hit)`；
- 排班层保留分量汇总，不混量纲。

### Phase 3：建立分量策略入口，不接公式（已完成）

当前接口：

```rust
pub enum ScoringPolicyId {
    ControlInjectRawSumV0,
}

pub struct TradeManuEfficiencyComponents {
    pub trade_eff_pct: f64,
    pub gold_manu_eff_pct: f64,
    pub battle_record_manu_eff_pct: f64,
    pub trade_station_count: u8,
    pub gold_line_count: u8,
    pub battle_record_line_count: u8,
}

pub struct ComponentScore {
    pub policy: ScoringPolicyId,
    pub sort_key_pct: f64,
}
```

`search/control.rs` 通过 `current_control_inject_sort_score(...)` 保持旧 raw-sum 排序行为，同时明确它只是当前中枢候选排序策略。

### Phase 4：加当前行为锁定测试（已完成）

已完成测试锚点：

- `search::trade::tests::trade_search_score_is_paper_efficiency_sort_key`
- `search::manufacture::tests::manufacture_search_score_is_prod_total_sort_key`
- `search::power::tests::power_search_score_is_charge_speed_sort_key`
- `search::control::tests::control_search_score_uses_total_score_sort_key`
- `scoring::components::tests::control_inject_raw_sum_reports_policy_and_current_sort_key`

### Phase 5：分量化输出与锚点收敛（当前后续）

后续工作：

- CLI / JSON 输出中更明确展示贸易赤金订单效率、制造赤金效率、制造经验效率；
- 中枢输出显示 `ScoringPolicyId::ControlInjectRawSumV0` 或等价说明；
- 补齐公孙已给出的等效效率锚点，而不是补“平衡公式”锚点；
- 审计前端字段命名，避免 `composite` / `balanced` 这类误导词；
- 如果将来某个局部场景需要排序，新增命名 policy，而不是写匿名权重。

---

## 5. 当前审计结论

### 5.1 贸易搜索

文件：`crates/infra-core/src/search/trade.rs`

- `TradeSearchHit.score` 当前等于 `result.order_eff_total`；
- `trade_pct` 同样等于 `result.order_eff_total`；
- L3 shortcut 的 `trade_pct` 使用公孙等效效率；
- `gold_pct` 单独展示赤金相关解释分量；
- `effective_eff_multiplier` 仅为内部调试乘积，不作为用户侧最终倍率。

### 5.2 制造搜索

文件：`crates/infra-core/src/search/manufacture.rs`

- 单配方搜索按 `prod_total` 排序；
- 多产线模式按产线数加权输出当前排序结果；
- 后续重点是输出字段命名，避免把 `composite_score` 理解为跨域综合分。

### 5.3 发电搜索

文件：`crates/infra-core/src/search/power.rs`

- 当前排序只看 `charge_speed_pct`；
- `virtual_power_equiv` / `VIRTUAL_POWER_MANU_EQUIV` 只保留解释，不参与当前排序；
- 如果后续要排序虚拟发电价值，应新增明确 policy，而不是混入制造效率。

### 5.4 中枢搜索

文件：`crates/infra-core/src/search/control.rs`

- 普通中枢补位输出 `trade_inject` / `manu_gold` / `manu_br`；
- 当前 `Efficiency` 排序策略使用 raw-sum：`trade_inject + manu_gold + manu_br`；
- 该 raw-sum 是局部排序策略，不是公孙理论公式；
- `HrAndMood` 策略仍使用 `ancillary_score`。

### 5.5 排班汇总

文件：`crates/infra-core/src/schedule/base_rotation.rs`、`team_rotation.rs`

- `ShiftScores` 分开存 `trade_score` / `manu_prod_sum` / `power_charge_sum`；
- `weighted_*` 只做时长折算；
- 当前没有把三类硬合成一个全局分，这一点应保留。

---

## 6. Warning hygiene 记录

2026-06-23 已完成一轮安全 warning 清理，仅处理 unused import / unused variable / unused mut / 测试专用 import 下沉等。

未处理且暂不建议处理：

- `private_interfaces`：涉及内部数据结构可见性，可能影响 API / serde 边界；
- `dead_code`：多为未来机制或 schema 预留；
- 预留字段未读：如 system schema、导出输入结构等；
- 未来 global effect 常量未使用：等待 Phase 4 global effect 收拢。

---

## 7. 风险和防护

| 风险 | 防护 |
|------|------|
| 又把展示字段用于排序 | 抽 sort key 函数 + 测试 |
| 又发明跨域综合权重 | 只允许命名 policy，且保留原始分量 |
| 改字段名导致前端误读 | `FRONTEND_CLI.md` 和 JSON 字段同步说明 |
| 体系 priority 与局部排序冲突 | priority 是硬先验，局部 policy 只处理补位或同层候选 |
| 性能退化 | policy 必须为 O(1) 纯函数，不引入全局笛卡尔积 |

---

## 8. 建议验收标准

- [x] `docs/机制/SCORING_MODEL.md` 完成首轮审计表；
- [x] 主要 score 字段注释与实际代码一致；
- [x] 所有搜索模块有明确 sort key 入口或明确注释；
- [x] `scoring` 模块不再宣称等待贸易-制造平衡公式；
- [x] 当前核心测试通过；
- [ ] CLI / 前端字段进一步拆出 trade_gold_order / manu_gold / manu_br 等语义名；
- [ ] 公孙等效效率锚点继续补入 shortcut / verify / unit anchors。
