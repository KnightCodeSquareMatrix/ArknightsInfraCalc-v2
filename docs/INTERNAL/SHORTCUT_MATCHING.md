# trade/shortcut L3 匹配地图

> 文件：`crates/infra-core/src/trade/shortcut.rs`  
> 数据：`data/trade_shortcuts.json`  
> 设计背景：`EFFECT_ATOM_DESIGN.md` §8.7

## 求解器中的位置

`solver.rs` 在 L1（`apply_trade_phases`）结束后：

1. 取 `order_eff_pre = ctx.order_eff_total()`
2. 若赤金订单 → `resolve_trade_shortcut(..., &layout.global_inject)`
3. 命中则走 L3：`sc.build_mechanic_result` + 表内 `trade_pct` / `gold_pct`，**覆盖** L2 的 `resolve_order_mechanic`
4. 未命中则走 L2 `order_mechanic::resolve_order_mechanic`

互斥违规在 solver 入口直接 `Err`（`trade_station_exclusive_violation`）。

## 链段注册表（`trade_segments.json`）

数据：`data/trade_segments.json`；代码：`trade/segment.rs`（求解命中）、`search/role_pick.rs`（meta 落位 fallback 链）。

| 字段 | 含义 |
|------|------|
| `segments[]` | producer 条件 + `consumer` 种类 + `shortcut_id` + `priority` |
| `roles[].pick_steps` | meta 站落位顺序：`segment` → `shortcut` → `unfiltered` |

**Producer**（`GlobalInjectManifest`）：`haru_e2_in_control`、`karlan_precision`。

**Consumer**（Rust 匹配器）：`docus_syracusa`（但书+伺夜+贝洛内）、`ling_jie`（灵孑银崖）。

**`roles.docus` fallback 链**：

1. `segment/docus_syracusa`（仅 `haru_e2_in_control` 时尝试）→ `gsl_docus_syracusa` trade=90
2. `shortcut/gsl_docus_solo` → L1 动态 `order_eff_pre`
3. `unfiltered` → 无 filter 全池（公孙盒无但书时 Plain）

`resolve_trade_shortcut` 在巫恋/可露之前调用 `match_registered_trade_segment`（按 `priority`）。

## 成套方案认领（`base_systems.json`）

数据：`data/base_systems.json`；代码：`layout/system.rs`（`claim_base_systems`）。

在 `assign_shift` **开头**（高峰班）按 `priority` 认领跨设施固定组合：先占 `control` / `trade_post` 等空房，写入 `used`；后续设施贪心跳过已占房间。中枢若只钉了体系内 1 人（如海铃），`assign_control` 会**补满剩余席位**而非整房重搜。

来源：公孙长乐工具人表（`scripts/build_base_systems_from_gongsun_xlsx.py` 维护小目录）。`exclusive_group` 互斥（如 `meta_chain`：叙拉古/喀兰/推王/怪猎四选一）；`pick_one` 在认领时按顺序取盒内第一个可用干员（如裁缝β四选一）。贸易 L3 锚点仍在 `trade_shortcuts.json`。

## 匹配优先级（`resolve_trade_shortcut`）

```
互斥检查 → None（solver 层已 Err，此处双保险）
    ↓
链段表 match_registered_trade_segment（docus_syracusa / ling_jie）
    ↓
但书单走 match_docus_solo_shortcut
    ↓
巫恋核 match_witch_group_shortcut
    ↓
可露希尔分档 match_closure_shortcut
    ↓
None → 走 L2
```

## 同房互斥（`trade_station_exclusive_violation`）

以下组合**禁止同站**（搜索 / 轮换 / 求解均拒绝）：

| 规则 | 函数 | 说明 |
|------|------|------|
| 但书 × 巫恋侧 | `docus_tailor_exclusive_violation` | 但书（合同法/违约）与巫恋低语 / 龙舌兰投资 / 裁缝 αβ 不得同房 |
| 佩佩 × 效率人 | `pepe_station_trade_eff_violation` | 佩佩独占站时，他人不得有 `constant` 阶段 `AddFlatEff` |
| 但书 × 可露希尔 | `trade_station_exclusive_violation` 内 | 违约链与特别订单 |
| 巫恋低语 × 可露希尔 | 同上 | 低语清零与特别订单 |

**巫恋侧判定**（`room_has_witch_side_group`）：精二巫恋且 `PeerEffAbsorb rate>0`；或龙舌兰投资；或同房有裁缝 α/β。

## 但书单走（`gsl_docus_solo`）

- 条件：`is_docus_solo_station` — ≥3 人、有但书机制 buff、无巫恋侧。
- `trade_pct` **运行时覆盖**为 L1 算出的 `order_eff_pre`。
- `gold_pct` 固定锚点 `DOCUS_MECHANIC_GOLD_PCT = 55`（纸面工具效率 ×1.55 机制等效）。

## 巫恋核（`gsl_witch_*`）

前提：`has_witch_e2`（精二巫恋 + `rate_per_peer > 0`），且同房**无**但书、**无**可露希尔。

`classify_witch_room` → shortcut id：

| 条件 | shortcut id |
|------|-------------|
| 龙舌兰精二 + 裁缝 β（第三人） | `gsl_witch_long_beta` |
| 龙舌兰精二 + 裁缝 α（无 β） | `gsl_witch_long_alpha` |
| 龙舌兰精二 + 空白第三人 | `gsl_witch_long_blank` |
| 龙舌兰精0 + 空白第三人 | `gsl_witch_long0_blank` |
| 无龙舌兰精二 + β + 空白第三人 | `gsl_witch_beta_blank` |

表内 `trade_pct` / `gold_pct` / `unit_trade_anchor` 为公孙工具人锚点；L1 仍算 `order_eff_pre` 供对比。

## 灵孑银崖（`gsl_ling_jie_yaxin`）

- 条件：`inject.karlan_precision()` 有值（中枢灵知 E2 已 resolve）+ 孑带 `trade_ord_limit_count[000]` + 同房有 **银灰** 与另一名 **`cc.g.karlan` 干员**（崖心/角峰/讯使等）。
- `trade_pct` 固定 **125.0**（工具人表）；`gold_pct` 0。
- 与巫恋/可露希尔/但书无互斥；未命中时 L1 裸算约 132%（见 `需要完成的干员建模.md` §一）。

## 可露希尔分档（`match.kind == "closure"`）

- 条件：`has_closure`、无巫恋低语、无但书。
- 在 `trade_shortcuts.json` 中筛 `match.kind == "closure"` 的条目。
- 选 `station_trade_pct` 与 `order_eff_pre` **距离最小**的档。
- 若最小距离 **> 25** → 不匹配（回退 L2）。
- 典型 case_id：`reg_gsl_closure_tier90` / `tier80` / `tier60`。

## 数据文件字段

| 字段 | 含义 |
|------|------|
| `id` | 回归 `expect_shortcut`、solver 输出 `trade_shortcut` |
| `trade_pct` / `gold_pct` | L3 锚定效率（但书 solo 的 trade 运行时覆盖） |
| `tailor_tier` | 裁缝档 → `GoldDistribution` |
| `match.kind` | `closure` 等匹配器类型 |
| `match.station_trade_pct` | 可露希尔分档纸面 trade% 锚点 |
| `unit_trade_anchor` / `unit_gsl_gold_anchor` | 产量层锚点（L3 未展开巫恋核等） |

## 回归夹具映射

`verify_cmd` 按 `expect_shortcut` 前缀选夹具（**不是** CSV `operators` 列）：

| `expect_shortcut` 前缀 | 夹具函数 |
|------------------------|----------|
| `gsl_witch_*` | `witch_fixture(shortcut_id, level)` |
| `gsl_docus_*` | `docus_fixture(case_id, level)` |
| 其他已接线 closure | `closure_fixture(case_id, level)` |

未接线 case 打印 `skip ... (fixture not wired)`。夹具定义：`infra-cli/src/verify/fixtures.rs`。

## 改 L3 时检查清单

1. `trade_shortcuts.json` 条目与 `id` 一致
2. `shortcut.rs` 匹配条件（尤其互斥与 `classify_witch_room`）
3. `verify/fixtures.rs` 若新族需要新夹具
4. `REGRESSION_CASES.csv` 的 `expect_trade_pct` / `expect_gold_pct` / `expect_shortcut`
5. `cargo run -p infra-cli -- verify --case <id>`
