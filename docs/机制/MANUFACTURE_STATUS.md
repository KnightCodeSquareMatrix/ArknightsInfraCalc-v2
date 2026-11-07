# 制造站求解状态

> 文档角色：canonical
> 生命周期状态：current
> 摘要：裁决制造站输入、阶段解释、产线聚合、仓储和班次心情结算边界。
> 领域键：facility.manufacture
> 当前真源：self
> 代码范围：`crates/infra-core/src/manufacture/{input,interpreter,solver}.rs`; `crates/infra-core/src/pool/manufacture.rs`; `crates/infra-core/src/search/manufacture.rs`; `crates/infra-core/src/layout/assign/manufacture_fill.rs`; `data/prts_manufacturing_skills.json`
> 代码锚点：`crates/infra-core/src/manufacture/interpreter.rs::apply_manu_phases`; `crates/infra-core/src/manufacture/solver.rs::{solve_manufacture,solve_manufacture_with_shift,evaluate_manufacture_lines}`; `crates/infra-core/src/search/manufacture.rs::search_manufacture_triples`; `crates/infra-core/src/pool/manufacture.rs::build_manufacture_pool`; `crates/infra-core/src/layout/assign/manufacture_fill.rs::assign_manu_room_with_anchors`
> 关联链路：`docs/模块/制造求解.md`; `docs/调用链/技能到房间结算.md`; `docs/调用链/跨设施状态刷新.md`
> 核验基线：`947f1a2`

## 责任边界

制造 solver 接收已经确定的房间、配方、实际三人组、LayoutContext 和可选班次时长，输出生产效率、单位日产量、仓库上限、心情消耗和分解。它不决定 control/trade 的候选，也没有贸易站的订单分布、gold flow 或 shortcut。

## 结算顺序

```text
ManuRoomInput
  -> ManuContext::from_room
  -> apply_manu_phases（借用预编译 atom，按阶段稳定归并）
  -> RecipeEff + RecipeLimit + state/global inject
  -> solve_manufacture[_with_shift]
  -> ManuResult / ManuProdBreakdown / ManuStorageBreakdown
```

`apply_manu_phases` 借用各来源的预编译 atom，按阶段、owner 顺序与原子序号稳定归并执行；处理制造专属 tagged inject、配方条件、PeerAbsorb、仓储与心情。`solve_manufacture_with_shift` 额外在班次/心情 gap 上求值；静态探索传 `None` 或调用 `solve_manufacture`。多线由 `evaluate_manufacture_lines` 汇总，不能由 CLI 手拼。

仓容反馈直接读取每名干员 `RecipeLimit.for_recipe(active_recipe)` 的净增加量，不再维护平行的仓容贡献字段。红云逐人截零后每格 +2%；泡泡逐人截零后按 >16 格取高档，覆盖红云的优先级保持；设施基础仓容和其他配方未生效的仓容不进入反馈。当前配方生效的限定仓容必须计入，例如稀音作战记录 +12。该修复为精确机制修复，完整、轻量、定时与 Bake 生成共用解释器；旧口径 Bake 行需要重新生成。

制造暖机按 `Action::AddEffRamp` 从每班进驻起点 `t=0` 连续计时。首小时型在 `0≤t≤1` 保持 initial（20% 或 15%），之后按 `initial + per_hour × (t−1)` 线性增长并封顶 25%；阿罗玛首小时全程为 0，之后按 `2 × (t−1)` 线性增长并封顶 20%。有班长使用真实 f64 小时，不取整、不截断 24h；无班长默认 20h 积分窗口且保留静态心情口径。非有限班长报错；有限零/负班长保持静态输出接口，取进驻起点加成而不积分。来源：用户 2026-10-29 裁决“线性增长，第1个小时加成为0”，及固定知识库制造站「时间爬升」与五条暖机技能原文；运行时映射为 `skill_table.json` 的 `add_eff_ramp` → 制造解释器。

轻量排序与完整结果共用连续积分 owner。无技能面非线性消费者时，暖机在真实 elapsed 区间解析平均后只解释一次；心情区间不重启暖机。有 `PeerSkillEffSum` 分桶时，先切首小时、各封顶时刻、铅踝心情条件/桶阈值及心情封顶，再在原解释器真正执行分桶前记录同一个 selector 的 base/step，按原执行顺序逐消费者细分真实跨桶时间。最终各段中点原解释器的原始 f64 按时长加权，汇总后统一转 Efficiency；桶公式仍只在解释器。班末另算终态仓库，心情消耗仍沿用最后有效区间的接口语义。在至少两个可表示内点的区间内执行数学分段积分；不足两个内点的微小段保留真实时长，暖机取解析区间平均、其余动作在可用内点或最近中点端点求值。这是 f64 时间分辨率的边界，非线性微小段仍有舍入误差；未支持的暖机效率消费者、不稳定执行键、不可表示或过大的阈值枚举明确报错，不静默改用平均近似。无暖机组合保留原轻量/完整求值和心情分段行为，定时观察在 atom 热路径编译消除。

暖机映射的五条 `AddEffRamp` 置于 EffVar/order9，技能面分桶置于 order30；先写齐直接技能面贡献（含暖机和铅踝的心情扣减），再读取他人技能面，修正同 order10 的后置暖机漏读及原 order20 心情扣减未被读取的问题。解释器仍按原阶段与 owner/atom 归并，未按干员名补偿。

<!-- knowledge:historical -->
## 连续暖机实施时的历史验收

以下数量、旧12h政策与预算失败属于当轮binary/input，不解释当前搜索。2026-10-31已删除后续工作预算和制造≥12h排除；当前责任见[定时换班](../排班/定时换班.md)及[删减导航](../指南/检查与删减导航.md)。

新冻结局部验收（CLI `7695096fd...`）：原55×3独立解析及225输入保留/重跑，新增129输入，总1122次full中1050成功、72预期非有限错误，数学千分值和公开f64合同零差异。1050次search中798获准并通过真实light→full效率断言及best/top完整字段精确比较；252次20/30h搜索因原有负心情扣减技能`>=12h`政策排除，不计为light/full验证通过。该历史门禁当时位于`search/manufacture.rs`，阈值12.0，现已删除；例如阿罗玛+槐琥+铅踝mood20/20h full为1.620，search evaluated=0。P1静态0h六序final1.530、P2芬+槐琥nextafter1三配方两序final1.420已验证，指定P1/P2静态复审闭合。证据`target/codex-runs/perf-final-20261028/correctness-warm/p1p2-final/{report.md,summary.json,search-policy-source.json}`。范围限点名真实技能、心情与时长，不证明私有light独立数值、全技能/全局注入、个体耗尽或排班；普通段积分与f64微段舍入边界见上文。

带班长且查询池含暖机 atom 时，Bake 整个查询回 live，并记 `ramp`；无班长的固定 20h 行按新积分口径重新生成与全量对账。该门禁不删除候选，只影响暖机查询。当前状态：新冻结连续暖机局部数值验收与指定P1/P2静态闭合完成；最终新语义隔离Bake与统一包已完成：29,783,417行/3,292兼容查询零差异、2,148查询跳过，CLI exit0且但书PASS；152盒147完成、5既有预算失败逐字同基线，新增/移除0，raw same5/changed147。全精二无断言profile11叶/rotation10叶变化、MAA相同；带断言同旧shift2预算失败，均无输出。5949叶差异归因已闭合，不称所有gate全绿。 不把局部结果认证为所有实数区间或全排班。

## 搜索与排班

工程机器人保留“设施等级产生资源 → 机械辅助读取资源”的结算链。机器人消费产生的效率归入设施派生效率 `layout_eff`，仍计入至简自身总生产力，但不进入槐琥的其他干员技能面读数，也不被同房清零清除；其他资源读取仍沿用原分类。来源：用户 2026-10-31 裁决槐琥误读属于至简工程机器人概念简化的问题，并要求一并修复。α 每 16 个、β 每 8 个机器人加 5%，整档向下取整；设施等级 producer 上限 64。

`build_manufacture_pool` 将 OperBox 的 facility binding、tier 和 `SkillTable` 映射为 `ManuPoolEntry`；缺失实例/buff 记录 skip，不是白板高效技能。`search_manufacture_triples` 先按 `full_pool` 过滤，再对单配方做 `C(n,3)` 组合；`ManuSearchRecipeMode::Lines` 分别求赤金和作战记录线并按线数合并。排班填房使用 `manu_options`、Plan anchor、`used` 和最新 resolve 快照。

## 失败与回退

配方/等级/人数非法直接返回错误；单配方无解返回无 hit/错误，由上层决定是否换配方。心情不足返回带gap的真实结果，排班终态作质量提示，不恢复工作预算筛选或拒绝；制造 solver 不修改宿舍。全局资源未刷新时检查 `resolve_base` 与 cross-facility 链，不在制造结果出口补百分比。Bake 不兼容时回退 live search。

核验入口：`layout eval --explain-json`、`infra-cli bench`、制造搜索报告和 `plan --assert-invariants`。机制数字来自知识库桥接；运行时 JSON 只作为输入载体。
