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
expected_output = unit_output_per_day × total_efficiency × minutes / 1440
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

bake schema v13 直接保存 `*_efficiency_millis: i32`，加载后还原为
`Efficiency`。因此 bake 的排序、序列化和运行时比较不再依赖浮点数，也不会受平台
浮点尾差影响。

## 3. 三类生产域

### 3.1 统一展示口径

贸易和制造房间统一展示三层效率分量：

```text
base_efficiency    = 1.000 + nonzero_mood_operator_count × 0.010
global_efficiency  = 中枢或其他设施写入当前生产域的全局加成
skill_efficiency   = 当前房间干员组合技能最终结算的效率
total_efficiency   = base_efficiency + global_efficiency + skill_efficiency
```

`occupancy_efficiency` 仍作为诊断分量返回，但它已包含在 `base_efficiency` 中，不能再次相加。
`control_efficiency` 是贸易接口中的历史字段别名，新的统一接口使用 `global_efficiency`。

### 3.1.1 贸易全局效率的具体口径

贸易域的 `global_efficiency` 专指中枢来源的贸易站全局效率效果：阿米娅、明椒、阿斯卡纶、若叶睦和望提供的贸易站 `+7%` 效率加成。五者属于同一个贸易全局效果族；同一班中若有多个来源，只取最高的 `7%`，不叠加为 `14%`、`21%` 等。该分量应进入 `global_efficiency`，不能重复计入 `skill_efficiency`。

八幡海铃的效果不属于贸易全局效率：她按当前贸易站内实际进驻的叙拉古干员数量，为每名叙拉古干员提供 `+5%`，属于当前房间技能效率，计入 `skill_efficiency`。其他只作用于当前房间或实际成员的跨设施响应也不能按全局 `+7%` 处理。

### 3.1.2 读数异常时先核对分量

当结果看起来“过高”或“过低”时，先把同一份结果拆成 `base_efficiency`、`global_efficiency`、`skill_efficiency` 和 `total_efficiency`，再比较数值。不要把一份文档中的技能分量与另一份文档中的总效率直接比较，也不要把订单上限、仓库容量或心情恢复量直接加进效率。

贸易域还要注意跨设施效果的落点：某些中枢效果会在贸易解释器开始前写入目标干员的运行时效率或订单上限，因此它可能已经包含在 `skill_efficiency` 或最终订单上限中，不能再按原始技能描述额外加一次。应以 `PaperTradeEfficiency` 的分量和最终 `total_efficiency` 为准，并沿 `source buff -> operator instance -> EffectAtom -> phase -> output field` 核对一次。

喀兰组合是标准例子：策略记录的 `129%` 是技能结算，三级站三人满员的基础项是 `103%`，所以统一总效率是 `2.320`。灵知的 `-15%/+6` 还会改变银灰的有效效率和孑的订单上限压缩，不能只把 `-15%` 当作一个独立的总效率减项。

实际产出不再伪装成效率倍率：

```text
output = unit_output_per_day × total_efficiency × work_time / 24
```

`final_efficiency` 不再出现在公开 JSON/CLI 协议；贸易候选排序使用不含特殊单位产出倍率的
`total_efficiency`。特殊组合的单位产出倍率只通过 `unit_output_per_day × total_efficiency`
进入实际产出，不得改变房间总效率排序。

### 3.2 贸易

```text
total_efficiency = base_efficiency + global_efficiency + skill_efficiency
trade_output_per_day = trade_unit_output_per_day × total_efficiency
gold_output_per_day = gold_unit_output_per_day × total_efficiency
```

普通龙门币贸易单位日产出：一级 `10000`，二级 `10141`，三级 `10265`。仅三级贸易站可选择
“开采协力”：每 2h 消耗 `2` 源石碎片、获得 `20` 合成玉，因此每 `1.0` 效率、24h 的基准产出为
`240` 合成玉，订单消耗为 `24` 源石碎片。它没有随机订单档位，也不产生龙门币。特殊组合直接提供其贸易和赤金单位日产出，不把倍率乘回 `total_efficiency`。

普通赤金订单分布按贸易站等级为：一级 `2 金 100%`；二级 `2 金 60% / 3 金 40%`；三级
`2 金 30% / 3 金 50% / 4 金 20%`。二级的 `60% / 40%` 只更新订单机制分布，不改变二级
社区单位产出锚点 `10141`。本次变更仅涉及普通赤金订单，源石订单规则不在本次范围内。

当前单位产出锚点：

| 组合 | 贸易单位日产出 | 赤金单位日产出 |
|---|---:|---:|
| 可露希尔 | 12000 | 2000 |
| 龙舌兰 + 裁缝 β | 12739.73 | 2328.77 |
| 巫恋 E2 + 龙舌兰 E2 + 裁缝 α | 12030.46 | 1675.127 |
| 巫恋 E2 + 龙舌兰 E0 + 裁缝 α | 11331.04 | 957.76 |
| 巫恋 E2 + 龙舌兰 E0 + 裁缝 β | 11575.34 | 1164.384 |
| 龙舌兰 + 但书 + 纯效率 | 16637.17 | 707.96 |
| 但书 E2，一级站 | 20000 | 0 |
| 但书 E2，二级站 | 18591.55 | 0 |
| 但书 E2，三级站 | 15929.2 | 0 |
| 但书 E0，一级站 | 15000 | 0 |
| 但书 E0，二级站 | 14366.197 | 0 |
| 但书 E0，三级站 | 13097.345 | 0 |

例如三级普通站中，组合总效率为 `2.020`、可露希尔贸易单位日产出为 `12000`，则贸易产出为：

```text
12000 × 2.020 × 工作时间占比
```

贸易产出和订单赤金消耗分别返回，不能把订单消耗折算成赤金产出。

完整轮换报告额外返回 `daily.production`。它按各班次、各房间的产出率累加后，使用
`sum(班次时长)` 归一到 24h 平均值；`lmd` 是龙门币订单产出，`pure_gold` 是赤金制造产出，
`battle_records` 是作战记录，`orundum` 是贸易站「开采协力」合成玉订单产出，
`originium_shards` 是制造站源石碎片产出。贸易订单交付赤金和源石碎片投入不进入产物汇总。

等效效率只用于把特殊组合与普通纯效率干员比较。对每条产出分别计算：

```text
trade_equivalent_efficiency = total_efficiency × trade_role_multiplier
                              - base_efficiency
                              - global_efficiency
gold_equivalent_efficiency = total_efficiency × gold_role_multiplier

combined_output_value = total_efficiency × trade_role_multiplier
                      + total_efficiency × gold_role_multiplier
```

贸易等效效率用于比较“需要多少普通贸易技能效率”，所以扣除基础和中枢加成。赤金等效效率只如实还原特殊组合具备的赤金产能，不扣除基础效率、中枢加成或贸易订单消耗。贸易和赤金各有独立的 `role_multiplier`、`equivalent_efficiency`、`unit_output_per_day` 和实际产出。
`combined_output_value` 是用户裁决的贸易:赤金 `1:1` 归一化单房诊断值，不是生产效率，不进入默认贸易排序、`DailyTotals` 或通用 solver comparator。

### 3.3 制造

```text
base_efficiency = 1.000 + nonzero_mood_operator_count × 0.010
total_efficiency = base_efficiency + skill_efficiency + global_efficiency
output = unit_output_per_day × total_efficiency × work_time / 24
```

制造“源石材料”配方的 `unit_output_per_day` 为 `24` 源石碎片：每个碎片无加速生产耗时 1h，消耗
`2` 固源岩与 `1600` 龙门币，或以 `1` 装置与 `1000` 龙门币替代。这里的 `24` 与贸易站开采协力的
`240` 合成玉是不同产物、不同单位，不能混用。

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
- 排班输出只复用带有完整纸面、人头/工位和最终效率分量的当前快照；旧 assignment 的不完整快照直接重新结算，不从 `final_efficiency` 反推缺失分量。
- 贸易、制造、发电每日汇总分开输出，不存在匿名跨域总分；具名、可分解且显式携带权重的
  候选排序 policy 不写回这些汇总字段。

## 5. 对外字段

| 域 | 最终字段 | 主要分解字段 |
|---|---|---|
| 贸易 | `total_efficiency` | `base_efficiency`、`global_efficiency`、`skill_efficiency`、贸易/赤金单位产出、贸易/赤金等效效率、实际产出 |
| 制造 | `total_efficiency` | `base_efficiency`、`global_efficiency`、`skill_efficiency`、配方单位日产出和实际产出 |
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

 - `REGRESSION_CASES.csv` 的旧列仍用于内部回归兼容；新增回归应锚定 `total_efficiency`、单位日产出和实际产出；
- `UNIT_OUTPUT_ANCHORS.csv` 独立锚定社区单位产出；
- `verify --all` 同时验证最终效率、机制解释、规则 ID 与单位产出；
- bake 需要 schema v13，旧 schema 不兼容，必须重新生成；
- CLI 文本和 CSV 的效率列固定三位小数，JSON 效率为已量化的数值。
