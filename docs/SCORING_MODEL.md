# 评分口径审计

> 文档角色：canonical
> 生命周期状态：current
> 领域键：scoring.policy
> 当前真源：self
> 摘要：裁决各域排序 policy 和评分分量

> 实现快照：直接效率硬切版（2026-07-11）
> 完整数值架构见 [EFFICIENCY_MODEL.md](EFFICIENCY_MODEL.md)。

## 1. 生产域排序

| 模块 | 排序字段 | 单位 | 说明 |
|---|---|---|---|
| 贸易搜索 | `total_efficiency` | 直接效率 | 新接口返回 `total_efficiency` 与贸易/赤金单位产出；特殊单位产出倍率只进入实际产出，不改变房间效率排序 |
| 制造搜索 | `ManuSearchHit.final_efficiency` | 直接效率 | 单线为房间效率，多线为各产线效率和 |
| 发电搜索 | `PowerSearchHit.final_efficiency` | 直接效率 | 基础、技能与爬升后的充能效率 |

三者都使用 `Efficiency` 千分整数排序。不存在生产域匿名 `score`，也不存在
`trade_pct`、`prod_total`、`charge_speed_pct` 的输出别名。

## 2. 排班汇总

`ShiftEfficiencies` 分别保存：

- `trade_efficiency`；
- `manufacture_efficiency`；
- `power_efficiency`。

每项是同类房间直接效率之和。`weighted_*` 按分钟执行整数时长折算，
`DailyTotals` 仍分开保存贸易、制造和发电，不做跨域相加。

`RoomEfficiencySnapshot` 保存 solver / search 的直接效率结果，避免 CLI 或排班层再次
解释公式。手写 assignment 没有快照时才调用相应 solver 重算。

### 2.1 动态 producer 联合候选的具名加权 policy

用户于 2026-07-27 裁决：比较已经完整 resolve 的动态 producer 联合候选时，贸易与制造按显式
`TradeManufactureWeightedEfficiencyV1` policy 合并排序：

```text
trade_total = Σr trade[r].total_efficiency
manufacture_total = Σr manufacture[r].total_efficiency
policy_sort_key = trade_weight × trade_total
                + manufacture_weight × manufacture_total
```

当前默认 `trade_weight : manufacture_weight = 1 : 1`。该比值是可调整的产品 policy 参数，
不是房间效率公式或永久常量；未来修改必须显式记录生效权重，并进入 policy/config 与 Bake
指纹，不能散落在 solver、Plan 或 CLI 中。布局中的贸易/制造房间数量负责宏观产出结构，
动态 producer 联合求解器只在给定布局内使用当前权重选最高加权总和。该裁决不自动扩张到
其他 System / Rule alternative 的选型目标。

此 policy 只消费分别结算的直接效率，不创建新的房间效率量纲，也不改变 `DailyTotals` 分域
输出。发电效率不进入加权总和，只作为布局约束和生产 response 的支持状态。required anchor、
体系原子性和人员互斥先作为 hard constraint 过滤；加权总和相等时再使用稳定 tie-break。

`TradeManufactureWeightedEfficiencyV1` 已作为动态 producer prefix 的候选 policy 接入
`AssignBaseOptions`：每个完整候选刷新贸易/制造快照后按上述公式比较，旧 raw-inject 上界在该
policy 下禁用。公开默认仍为 `ControlInjectRawSumV0`；`plan` 和 `layout team-rotation` 只有显式
传入 `--prefix-shadow-out` 才会另跑 V1 并记录差异，不替换默认结果。普通中枢补位继续使用
`ControlInjectRawSumV0`，因此当前 V1 保证范围是“每个 presence set 先由 V0 选一组普通补位，再
在这些完整前缀之间比较”，不是全部中枢组合或多房 exact joint；shadow 结果状态为
`BEST_FOUND`。exact joint 仍由
[动态 Producer A+](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md) 跟踪。

## 3. 非生产 heuristic

中枢普通排序当前仍使用具名 policy `ControlInjectRawSumV0`：

```text
trade_inject + manu_gold + manu_br
```

它是局部补位 heuristic，不是生产效率，也不进入三类每日总计。虚拟发电同样作为独立
资源注入制造 resolve，不预支成匿名综合分。中枢搜索结果通过
`breakdown.policy = ControlInjectRawSumV0` 与 `breakdown.policy_sort_key` 暴露排序依据，
不提供 `score` / `total_score` 或 `final_efficiency`。

### 3.1 戴菲恩跨中枢前缀比较

用户于 2026-07-17 裁决：比较包含戴菲恩的不同 control prefix 时，戴菲恩的动态贸易注入
分量使用各贸易房实际注入百分点之和：

```text
daifeen_trade_inject_sum = Σr (10 × n_g(r))
```

该分量属于当前具名中枢 policy，不是生产效率。`(3,0)` 与 `(2,1)` 在此前缀分量上同为 30，
但两者仍是不同的逐房 response signature；不得据此合并 CandidateRow、logical operator mask
或跨房状态。迁移到 `TradeManufactureWeightedEfficiencyV1` 后，戴菲恩注入只通过各房真实
`final_efficiency` 进入完整候选总和，不再作为跨 prefix 的最终 winner key；最终 winner 仍须
执行完整 live resolve。

## 4. 输出约束

- CLI / CSV：效率固定三位小数；
- JSON：效率是量化到三位小数的数值；
- 产量、赤金消耗、心情等非效率物理量保留各自单位；
- `rule_id` 只用于社区换算审计；
- `mechanic_equivalent_efficiency` 只作解释，不二次乘入最终效率。
- 排班 JSON 的班次结算字段为 `efficiencies`，不使用泛型 `scores`。
