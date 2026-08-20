# 直接效率与整数结算架构

> 文档角色：canonical
> 生命周期状态：current
> 领域键：scoring.efficiency
> 当前真源：self
> 摘要：裁决效率量纲、结构和输出边界

> 实现快照：implemented（2026-07-11）
> 范围：贸易、制造、发电、搜索、bake、排班快照、CLI / CSV / JSON

## 1. 唯一口径

生产域只公开直接小数效率：`1.000` 表示基础 `100%`，`1.550` 表示基础效率的
`155%`。调用方可以直接用它预估产出：

```text
expected_output = reference_output_per_day × final_efficiency × minutes / 1440
```

代码不再公开 `trade_pct`、`gold_pct`、`prod_total`、`charge_speed_pct`、匿名
`score` 等旧生产评分字段，也不保留 serde alias 或 fallback。机制层仍可按游戏数据的
百分数解释技能，但必须在 solver 边界转换成直接效率。

## 2. 数值表示与舍入

`infra_core::Efficiency` 是生产效率的唯一运行时类型：

- 内部 `i32` 千分位，`1000 == 1.000`；
- 输入在进入结算边界时四舍五入到三位小数；
- 加法、房间汇总和排序完全使用整数；
- 乘法使用 `lhs_millis × rhs_millis / 1000`，在结果处四舍五入；
- 时长折算先把小时转成分钟，再按整数比例四舍五入；
- `Display` / CSV / 文本固定输出三位小数；
- JSON 输出数值而不是百分比或原始千分整数，数值已量化到三位小数。

bake schema v12 直接保存 `*_efficiency_millis: i32`，加载后还原为
`Efficiency`。因此 bake 的排序、序列化和运行时比较不再依赖浮点数，也不会受平台
浮点尾差影响。

## 3. 三类生产域

### 3.1 统一展示口径

生产房间统一展示四个分量：

```text
operator_efficiency = 白字，干员本人及直接作用于干员的效率
station_efficiency  = 绿字，入驻人数与中枢/其他设施全局加成
total_efficiency    = 1.000 + operator_efficiency + station_efficiency
```

贸易站再按订单倍率生成实际产出效率：

```text
final_efficiency = total_efficiency × order_multiplier
equivalent_efficiency = final_efficiency - 1.000 - station_efficiency
```

没有订单倍率的制造和发电站使用 `order_multiplier = 1.000`，其 `final_efficiency` 等于
`total_efficiency`。跨站汇总只累加 `final_efficiency`，不重复累加白字或绿字。

### 3.2 贸易

```text
paper_efficiency
  = base_efficiency
  + occupancy_efficiency
  + skill_efficiency
  + control_efficiency

unit_output_multiplier
  = community_unit_output_per_day / reference_unit_output_per_day

final_efficiency = paper_efficiency × unit_output_multiplier
```

三级普通赤金贸易站基准日产出为 `10265`。但书、可露希尔、龙舌兰、巫恋等使用
社区已确认的单位产出换算，不还原逐笔订单逻辑。但书三级站即整个房间纸面效率整体乘
`1.550`；例如 `1.230 × 1.550 = 1.907`。

`mechanic_equivalent_efficiency` 只解释社区机制，不参与第二次乘法。
`equivalent_skill_efficiency` 也是从最终效率反算的展示分量。唯一排序和产出真源是
`final_efficiency`。

### 3.3 制造

```text
final_efficiency
  = 1.000
  + occupancy_efficiency   // 每名进驻干员 0.010
  + skill_efficiency
  + global_efficiency
```

单配方搜索按 `final_efficiency` 排序。多产线结果是各产线直接效率之和，仍保持制造域
量纲，不写入贸易或发电的结果字段。具名编制 policy 可以显式消费各域分量做候选排序，
但不改变这些分量的量纲或分域输出。

### 3.4 发电

```text
final_efficiency
  = 1.000
  + skill_efficiency
  + ramp_efficiency
```

发电搜索按直接充能效率排序。布局 `drone_cap` 表示无人机持有上限；满清理基建按 PRTS 为 235，承曦格雷伊「巡线框架」读取该值并按每 10 架 +1%（上限 25%）进入 `skill_efficiency`。`virtual_power_produced` 是独立资源，不匿名折入发电或
制造效率。

## 4. 数据流与职责

```text
L1 技能解释（原始机制数值）
  -> solver 边界量化为 Efficiency
  -> search hit / breakdown
  -> room efficiency snapshot
  -> shift room sum
  -> integer time weighting
  -> daily totals
  -> CLI / CSV / JSON decimal output
```

- `infra-core` 负责结算、量化、排序、汇总和产出预估。
- `infra-cli` 只负责加载与格式化，不补基础 `1.000`，也不拼百分比公式。
- `data/trade_shortcuts.json` 保存规则 ID、社区单位产出和机制等效效率。
- `RoomEfficiencySnapshot` 只保存新直接效率字段；旧 assignment 必须迁移数据。
- 贸易、制造、发电每日汇总分开输出，不存在匿名跨域总分；具名、可分解且显式携带权重的
  候选排序 policy 不写回这些汇总字段。

## 5. 对外字段

| 域 | 最终字段 | 主要分解字段 |
|---|---|---|
| 贸易 | `total_efficiency`（纸面）/ `final_efficiency`（订单倍率后） | `operator_efficiency`、`station_efficiency`、`order_multiplier`、`equivalent_efficiency` |
| 制造 | `total_efficiency` = `final_efficiency` | `operator_efficiency`、`station_efficiency` |
| 发电 | `total_efficiency` = `final_efficiency` | `operator_efficiency` |
| 班次 | `trade_efficiency` / `manufacture_efficiency` / `power_efficiency` | `room_lines`；每房输出 `operator_efficiency`、`station_efficiency`、`total_efficiency`、`order_multiplier`、`equivalent_efficiency` |
| 日汇总 | `trade` / `manufacture` / `power` | 各自为按时长加权后的直接效率 |

班次按时长折算后的字段固定为 `weighted_trade`、`weighted_manufacture`、
`weighted_power`。制造搜索 JSON 固定放在 `manufacture` 域下，最终值和各分量分别使用
`final_efficiency`、`base_efficiency`、`occupancy_efficiency`、
`skill_efficiency`、`global_efficiency`；不保留 `manu_*` 或百分比兼容字段。

生产候选若进入通用 `TeamCandidate`，最终值只写 `final_efficiency: Option<Efficiency>`；
附加解释值写入 `metrics[].value`，不再存在 `raw_score` / `decision_score`。中枢的
`ControlInjectRawSumV0` 通过 `policy` + `policy_sort_key` 单独表达，不冒充生产效率。
动态 producer 联合候选使用的 `TradeManufactureWeightedEfficiencyV1` 只消费贸易与制造分域
总和；当前默认权重为 `1:1`，未来调整权重不修改 `Efficiency`、房间快照或 `DailyTotals`
schema，也不自动成为其他 System / Rule alternative 的 comparator。

## 6. 回归要求

- `REGRESSION_CASES.csv` 直接锚定三位小数 `expect_final_efficiency`；
- `UNIT_OUTPUT_ANCHORS.csv` 独立锚定社区单位产出；
- `verify --all` 同时验证最终效率、机制解释、规则 ID 与单位产出；
- bake 需要 schema v12，旧 schema 不兼容，必须重新生成；
- CLI 文本和 CSV 的效率列固定三位小数，JSON 效率为已量化的数值。
