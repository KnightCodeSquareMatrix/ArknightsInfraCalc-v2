# 性能工程：候选池、组合搜索与 Bake 安全边界

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/QUALITY_AND_AUDIT.md；docs/SCORING_MODEL.md
> 摘要：记录当前性能事实、测量入口和风险边界

> 实现快照：当前实现与本地数据快照，核对日期 2026-08-31。本文解释性能事实，不改变搜索语义；未来设计只在文末单独标明。

本项目房间少、每房人数少，但候选池大。主要成本不是生成一个 `Vec`，而是对大量 `C(n,k)` 组合执行真实 solver，并在动态全基建上下文中重复这些搜索。

## 1. 先守住正确性边界

性能优化必须保持以下不变量：

1. Markdown 仍是业务语义真源；缓存不能改变机制。
2. `full_pool` 表示调用方需要完整合法候选范围，不能用 standalone 名单或 baked 子集偷偷替代。
3. shortcut 只结算实际组合，不能因为缓存中有高分行就绕过 producer、同房或互斥条件。
4. `used`、pinned anchor、room capacity、recipe/order、练度和 operbox 拥有情况必须在运行时重新检查。
5. 动态 context 不兼容时必须安全回退到实时搜索，不能使用基线旧分数。
6. Bake row 必须由同一真实 solver 离线生成；兼容命中时运行时复用该结果，不兼容时回退 live solver。Bake 不是第二套公式。

搜索 API 提供 `BakeMode::Auto / Disabled / Required`：生产默认 `Auto` 保持安全回退；回归中的
`Required` 遇到任何兼容、catalog、signature 或 row miss 必须失败，防止测试实际回退 live
仍被误报为 Bake 通过。`infra-cli bake` 生成后自动执行完整 signature/row 校验、每个
signature 首/中/尾 response 的 live solver 抽样对账与现有机制回归；`bake verify` 对既有
generation 重跑同一门禁。仓库发布入口再追加完整 release test suite。

维护期约束见 [MAINTENANCE_MODE.md](MAINTENANCE_MODE.md)，制造完整池边界见 [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md)，直接效率口径见 [EFFICIENCY_MODEL.md](EFFICIENCY_MODEL.md)。

## 2. 2026-07-14 组合规模快照

下表来自当日 `data/operator_instances.json` 的设施绑定去重计数；组合数用整数 `C(n,k)` 计算。它描述数据全集，不等于任一普通账号实际拥有数。

| 设施 | 去重干员数 | 典型组合 | 原始组合数 |
|---|---:|---:|---:|
| 控制中枢 | 55 | `C(55,5)` | 3,478,761 |
| 贸易站 | 77 | `C(77,3)` | 73,150 |
| 制造站 | 90 | `C(90,3)` | 117,480 |

若把 1/2/3 人房都算入单房列：

- 贸易每种订单：`C(77,1)+C(77,2)+C(77,3)=76,153`；
- 制造每种配方：`C(90,1)+C(90,2)+C(90,3)=121,575`。

标准最高合法练度夹具 `data/fixtures/243/operbox_full_e2.json` 当日共有 418 个 operbox entry；实际 `plan` 画像报告的可建模池为贸易 75、制造 90。数据全集与运行池不同是正常的：OperBox、练度解析和池过滤仍会排除不适用 entry。

### 快照生成口径

上述数字来自只读检查：

```text
operator_instances.json
  → 按 facility 过滤
  → 按 operator name 去重
  → 对去重人数计算组合数
```

它们不是性能 benchmark。运行耗时应使用 `profile layout-full`、pipeline 的 `[计时]` 行或带留痕的真实 `plan` 命令测量，见 [INFRA_CLI.md](INFRA_CLI.md)。

## 3. 工具人池与完整池

仓库存在两类容易混淆的候选范围。

### 3.1 Standalone / 工具人池

`data/standalone_roster.json` 是已知能独立工作的候选名录，适合 bench、普通单房探测和基线 Bake。它可以显著减小 `n`，但不能代表全部机制组合。

当前名录按配方的制造人数快照为：

| 配方 | standalone 人数 | 1/2/3 人列总数 |
|---|---:|---:|
| 赤金 | 15 | 575 |
| 经验书 | 22 | 1,793 |
| 源石 | 15 | 575 |

实际 Bake 还会经过 solver/合法性过滤，所以最终行数可以更少。

把当前工具人名录与设施绑定全集放在一起，可以直观看到它为什么值得做。下表只是“普通热路径”的组合上界示意；anchor、producer、role 或 `full_pool` 会按结构扩池，不能据此裁掉合法候选。

| 设施 | 数据全集 | 工具人名录 | 典型满房组合数：全集 → 工具人 | 组合缩减 |
|---|---:|---:|---:|---:|
| 控制中枢 | 55 | 10 | `C(55,5)=3,478,761` → `C(10,5)=252` | 约 13,804 倍 |
| 贸易站 | 77 | 21 | `C(77,3)=73,150` → `C(21,3)=1,330` | 约 55 倍 |
| 制造站 | 90 | 34 | `C(90,3)=117,480` → `C(34,3)=5,984` | 约 20 倍 |

制造实际还会按配方继续过滤到上表前述的 15 / 22 / 15 人。中枢则会在已有 pinned、动态 producer 或候选最低数量约束时主动跳过 standalone 收窄。这里的价值不是“白名单代替搜索”，而是先用小而高价值的候选宇宙快速完成普通路径，再在领域条件要求时恢复结构化完整性。

### 3.2 Full pool / 完整池

排班必须允许同房耦合、`atoms: []` 委托角色和非 standalone 体系参与。例如标准化、红云/泡泡、莱茵同房和自动化链不能靠“看起来像高效散件”的白名单覆盖。

[search/manufacture.rs](../crates/infra-core/src/search/manufacture.rs) 的 `full_pool` 是结构性安全标志；[bake.rs](../crates/infra-core/src/bake.rs) 当前明确要求 `!options.full_pool` 才使用制造 Bake。排班调用完整池时，旧 standalone Bake 必须回退，而不是静默裁剪。

## 4. 组合数如何被压缩

### 4.1 OperBox 与设施池过滤

`OperBox::trade_roster` / `manufacture_roster` 先按拥有和练度构造 roster，pool builder 再解析设施绑定、tier、buff 和必要过滤。通常运行时 `n` 小于数据全集。

### 4.2 Skill-less 中枢 filler

[search/control.rs](../crates/infra-core/src/search/control.rs) 不会让所有无技能干员参与 `C(account_size,5)`。无技能 filler 对中枢输出等价，只保留足够填满五个位置的一小段稳定候选；有技能、must-include 和 candidate requirement 成员仍完整保留。

### 4.3 Anchor 与 `used`

设施 fill 先按 `used` 删除已被其他房间占用的干员。must-include anchor 会从过滤 mask 中临时扣回，以允许搜索“保留已有成员并补齐房间”。这是可行域过滤，不是排序偏好。

### 4.4 `top_k` 不等于只计算 K 个组合

实时搜索通常仍生成并求值全部合法 `C(n,k)`，之后排序并截断为 `top_k`。因此把 `--top 20` 改成 `--top 10` 主要减少保留和下游候选，不会自动把 7 万次 solver 调用减半。

## 5. 当前 Bake 产物与 schema 事实

[bake.rs](../crates/infra-core/src/bake.rs) 当前声明：

```text
BAKE_SCHEMA_VERSION = 12
model = binary_single_room_combo_table
```

生成物包括：

- `operators.json`：E2 基准 roster 与设施；
- `combo_table.bin`：按 signature 排序的单房组合行；贸易行同时保存首批 room-local 机制签名；
- `manifest.json`：schema、生成器和输入 fingerprint；
- `summary.json`：行数和生成耗时摘要。

但是当前工作区的 [data/baked/manifest.json](../data/baked/manifest.json) 与 [summary.json](../data/baked/summary.json) 仍是旧 schema 5 快照：

| 本地旧 catalog 字段 | 值 |
|---|---:|
| operator_count | 109 |
| trade_signatures / trade_hits | 6 / 140,330 |
| manufacture_signatures / manufacture_hits | 9 / 2,711 |
| combo_table_rows | 143,041 |
| `combo_table.bin` 实测大小 | 21,547,906 bytes |

旧表平均约 150.6 bytes/row，但这是 schema 5 混合行格式的观测值，不能直接当作未来紧凑表的固定大小。

2026-07-17 的 schema v11 首批贸易 room-local 机制历史 catalog 在同一完整贸易 row universe 上生成
134,324 行，其中 30,306 行携带 45 个不同机制字段组合；生成约 1.47 秒。该数字不是结构化
response dictionary 的去重数。该数据只证明冷路径规模和
物化覆盖，不代表默认 `team-rotation` 已接入新的跨房条件查询，也不用于推导 200ms 目标已达成。

### 关键结论

代码期望 schema 12，而本地 manifest 是 schema 5。`validate_baked_catalog` 会报告 schema mismatch；运行时加载器把这一类错误视为缓存不可用并返回 `None`，搜索随后走实时路径。因此当前行为是“性能退化但结果安全”，不是使用旧表继续算。表中的大小仍是 schema 5 历史观测，不代表 v12 的实测大小。

## 6. Bake 冷路径

`infra-cli bake` 调用 `bake_catalogs`：

1. 加载 instances 与 skill table。
2. 构造 E2 level 1 rarity 6 的 Bake roster。
3. 为贸易站 level 1/2/3 × gold/originium 枚举单房组合。
4. 为制造站 level 1/2/3 × 三配方枚举 standalone 组合。
5. 每行调用真实 room solver，排除非法组合，保存最终效率及分解。
6. 按 `signature_key`、效率和姓名稳定排序。
7. 写二进制表、manifest 和 summary。

`--limit-per-signature` 会在写表前截断。需要完整账号过滤能力的 catalog 必须保持该值为 `null`；否则基线高分行因账号缺人被过滤后，表里可能根本没有本应出现的后续组合。

## 7. Bake 热路径

实时贸易和制造搜索分别从 [search/trade.rs](../crates/infra-core/src/search/trade.rs) 与 [search/manufacture.rs](../crates/infra-core/src/search/manufacture.rs) 尝试 `try_baked_*_search`。

热路径步骤是：

1. 检查当前查询是否与 Bake 模型兼容。
2. 验证 catalog schema、生成器与输入 fingerprint。
3. 用 operator name 建账号可用 bitset。
4. 在对应 signature 的已排序行中做 `row_mask ⊆ available_mask`。
5. 再检查 must-include 和 hit filter。
6. 收集到 `top_k` 后直接转换成 search report。

这里的 bitset 只加速成员过滤，不会决定技能效果。当前表加载后会从 `operator_indices` 重建每行 mask。

## 8. 当前兼容门禁

贸易 Bake 只在以下条件同时成立时使用：

- 没有 `must_operator_override`；
- mood 为 24；
- shift 为 24h；
- layout 与 Bake baseline 的非贸易全局上下文一致；
- trade global inject、producer flag 和 Karlan precision 等贸易效果一致；
- 候选池没有依赖 candidate projection 的 atom；
- 池内所有人都是 E2。

制造 Bake 当前要求：

- `full_pool == false`；
- mood 为 24；
- layout 与制造 baseline 一致；
- manufacture global inject 一致；
- 池内所有人都是 E2。

这些门禁看起来保守，但它们防止基线分数被错误复用于动态上下文。八幡海铃、戴菲恩、凛御银灰等跨站 producer，`OperatorInBase`/`OperatorInTrade` 条件，以及实际全基建 workforce 都可能让同一三人组在不同 context 下得到不同结果。

## 9. Fingerprint 与安全回退

当前 manifest 指纹覆盖：

- `operator_instances.json`；
- `skill_table.json`；
- `standalone_roster.json`；
- `trade_shortcuts.json`；
- `trade_segments.json`；
- `base_systems.json`；
- `layout/243_use_this_.json`；
- 生成 catalog 的 CLI executable bytes。

任一当前输入缺失、大小或 hash 改变，或 executable fingerprint 不一致，catalog 都会失效。该策略会因无关代码改动而保守重烘焙，但不会在机制代码变化后冒险使用旧分数。

OperBox 本身不在全局 catalog fingerprint 中；正确性由运行时 pool-name 覆盖检查、可用 bitset 和“全员 E2”门禁保证。若以后支持按账号/tier 烘焙，必须另加入规范化 operbox/pool fingerprint，不能只按姓名缓存。

加载器的安全策略是：

```text
catalog missing / stale / schema mismatch / generator mismatch / input mismatch
  → return None
  → runtime exhaustive search
```

因此优化 Bake 时，最重要的回归不是“命中率 100%”，而是“任何不兼容输入都可靠回退，且 Bake hit 与实时搜索结果一致”。

## 10. 常见性能误判

| 误判 | 实际情况 |
|---|---|
| `top_k=20` 所以只算 20 组 | 通常先算全部组合，再只保留 20 组 |
| 有 `data/baked/` 就一定走 Bake | schema、指纹或 context gate 任一不合即回退；当前本地 catalog 正是旧 schema |
| standalone Bake 可直接加速排班制造 | `full_pool=true` 会有意拒绝它，避免漏掉耦合组合 |
| 同一三人组可以永久缓存一个分数 | 动态 layout、global inject、候选投影和 shortcut producer 会改变结果 |
| `used` 只需最后查重 | 每个设施的可选池与后续搜索都受 `used` 影响 |
| 重新生成旧分数表即可解决全部耗时 | 中枢 `C(55,5)`、动态前缀和三班替补仍可能是主要成本 |

## 11. 性能调查顺序

1. 确认用户真实命令、layout、operbox、top_k 和输出模式。
2. 查看 pipeline/rotation 的阶段计时，先判断慢在 peak、gamma 还是最终评分。
3. 检查实际 pool 人数与 `C(n,k)`，不要只看 operbox 总人数。
4. 检查 Bake 是否真的通过 schema/fingerprint/context gate。
5. 检查是否因 `full_pool`、candidate projection 或动态 inject 正确回退。
6. 只有确认 solver 热点后，才考虑缓存中间结果或扩大 Bake；不能先加人员白名单。

验证和性能证据必须按 [AGENTS.md](../AGENTS.md) 的日志模板保存，重复运行不得覆盖旧日志。

### 11.1 stderr 阶段计时协议

`plan`、`plan.compute` 和共用 core 路径在请求线程内建立一次性能 trace。机器记录统一写入
stderr，每行以 `[perf] ` 开头，后接单行 JSON；stdout、profile、rotation 和 MAA 不承载计时。
当前协议 `schema_version=1`，事件固定为 `event=solver_stage`。

每条事件都有 `trace_id`、`layout`、`operbox`、`scope`、`stage`、`status` 和
`elapsed_ms`。`status` 只取 `ok` / `error`；时间统一为毫秒并保留到 0.001 ms。阶段枚举为：

| scope | stage |
|---|---|
| `plan_cli` | `resource_load`、`compute_plan`、`prefix_shadow`、`output_serialize_write` |
| `plan_compute` | `current_probe`、`explicit_invariants`、`profile_baseline`、`maa_build`、`total` |
| `rotation` / `rotation_abc` | `total`；`peak`、`shared_pool_split`、`gamma`、`shift_1`～`shift_3`、`shifts_and_postprocess` |
| `rotation_finalize` | `dorm_prepare`、`rescore`、`dorm_assign_and_totals`、`invariants`、`total` |
| `shift` / `shift_pipeline` | `plan_build`、`pipeline`；`plan_execute`、`pool_build`、`peak_prepare_control`、`peak_prefixes`、Recovery 各阶段、最终快照、会客室、排序、`total` |

`scope=search` 的 leaf 事件包含五类 operation metrics；`scope=shift_pipeline_resolve`、
`scope=rotation_resolve` 和 `scope=plan_compute_resolve` 的阶段事件只包含 resolve metrics，
用于观察 resolve 内部构造成本。其他父阶段事件只包含 wall time，避免同一工作在父子事件中重复累计。
没有活动时字段为 0，不省略。每个 operation 对象固定包含：

- `calls` / `wall_ms`：该阶段内完成的 leaf 调用数及 inclusive wall time；
- `prepare_ms` / `bake_lookup_ms` / `evaluate_ms` / `sort_topk_ms`：leaf 内候选准备、
  Bake 检查、live 求值、过滤排序截断的互斥子阶段；
- `generated_combinations`：搜索定义的合法组合规模；
- `candidate_attempts`：实际交给 live 求值循环的候选数；Bake 命中时为 0；
- `accepted_hits`：求值及过滤后保留的 hit 数，不等同于尝试数；
- `selected_assignments`：最终被贪心 assignment 选中的岗位数；只对发电搜索有实际意义，
  其他搜索为该操作返回的 top 结果数；
- `bake_hits` / `live_calls`：leaf 搜索的数据来源。

resolve operation 额外包含 `workforce_ms`、`layout_ms`、`mechanics_ms`、
`cross_facility_ms`、`support_ms`、`room_materialize_ms`，以及显式构造计数
`support_registry_loads`、`layout_clones`、`blueprint_clones`、`assignment_clones`、
`room_context_clones`。这些计数只覆盖源码中已确认的显式构造/clone，不是全局 allocator 统计；
隐式 derive clone、Rust 标准容器扩容和 Rayon worker 内部临时分配仍需外部 profiler。

唯一计数 owner 是单订单贸易搜索、单配方制造搜索、中枢组合搜索、发电 assignment 和公开
`resolve_base` wrapper。多订单 / 多产线聚合层和父阶段只报告 wall time，不重复累计 leaf 指标。
父子阶段以及不同 operation 的 `wall_ms` 都是 inclusive 观察量，不能相加后冒充总耗时；应在同一
`scope` 的互斥 sibling 间比较，并用父阶段 `elapsed_ms` 计算占比。

trace 由 RAII guard 保存并恢复线程局部状态，错误返回后不会污染常驻 worker 的下一请求。Rayon
worker 不逐组合写日志；调用线程只在 `par_iter` 完成后累计一次，因此日志锁不进入组合热循环。
测 warm worker 时应先发送一条不计入统计的同输入预热请求，再记录至少三条测量请求。

### 11.2 2026-08-31 构造与复制 drill-down

本轮没有启用 allocator 级 profiler；以下是源码显式 clone/构造和 release trace 的交叉结果，
不是把每次堆分配都精确折算成字节。

- `243 + full-E2` 的 `shift_pipeline_resolve / peak_prefixes` 阶段执行 75 次 resolve：
  产生 126 次 support registry parse/validate、759 次 `LayoutContext` clone、750 次
  room-context clone，以及 75 次 `Blueprint` 和 75 次 `BaseAssignment` clone。该阶段
  resolve wall time 为约 39.9 ms，其中 support registry load 约 22.3 ms，约占 resolve
  support 子阶段的 94%。`153 + high-owned` 的对应数据是 70 次 resolve、112 次 registry
  load、670 次 layout clone、664 次 room-context clone。
- 在包含完整 current rotation 的 `rotation_resolve / total` 聚合中，`243 + full-E2`
  共 537 次 resolve、1,045 次 registry load、6,175 次 layout clone、6,109 次
  room-context clone，以及 537 次 blueprint/assignment clone；resolve wall 约 292.5 ms，
  其中 registry parse/validate 约 173.6 ms，support 阶段约 188.4 ms。平均每次 resolve
  约 1.95 次 registry load、11.5 次 layout clone、11.4 次 room-context clone，并固定复制
  一次 blueprint 和一次 assignment。`153 + high-owned` 对应为 259 次 resolve、483 次
  registry load、2,734 次 layout clone，resolve wall 约 110.2 ms。
- `SupportRegistry::load_default()` 使用编译期字符串，但每次仍执行 serde parse 和 110
  行 validate；它当前不是文件 I/O，却确实是重复 CPU 和分配。它在单次 resolve 中通常出现
  两次：办公室前置条件一次、最终 support room 构造一次。
- `resolve_snapshot()` 只需要 `LayoutContext`，但当前先构造完整 `ResolvedBase`，包含
  support/trade/manufacture/power room vectors、`blueprint.clone()` 和 `assignment.clone()`，
  再执行一次 `layout.clone()`。因此每次 snapshot 至少支付一次最终 layout clone 和完整
  resolved-room 构造；这在 Peak prefix 中被 75 次重复。
- 制造搜索的每个候选都会从 pool entry 深复制 `ManuOperator` 的 name/buff/tags，之后
  evaluator 又复制 `ManuRoomInput` 的 operator vector；solver 再构造 runtime operators，
  `apply_manu_phases` 还复制 `ctx.operators` 并重新生成 names 和 atom owner strings。以
  `243 + full-E2` 核心 trace 为例，单次 current probe 有约 1,203,480 candidate attempts、
  其中制造 leaf wall 约 1,792 ms；其排序约 984 ms、求值约 760 ms。制造每个三人候选至少
  生成三份 pool-to-input operator 描述，mood-gap 长班次还会按窗口重复 runtime solve。
- 中枢与贸易也有同形的 per-candidate owned String/Vec 复制，但当前量级通常低于制造；
  发电在代表样本中只有几十到几百候选，clone 不是优先项。

因此当前最可信的根因排序是：

1. 制造 hits 全量物化和串行全排序；
2. 制造候选到 runtime solver 的多层 owned object 重建；
3. Peak prefix 重复完整制造后缀和 resolve；
4. resolve 内重复解析 support registry；
5. `resolve_snapshot` 为只读 context 构造完整 `ResolvedBase`。

低风险优化边界是先把 support registry 变成进程级只读缓存，并增加仅生成
`LayoutContext` 的 resolve owner；两者都不改变候选或公式。制造 operator/runtime 缓存、
top-K heap、Peak prefix 结果复用和 resolve context cache 需要分别证明 recipe、mood、
`used`、candidate projection、global inject、producer presence 和稳定 tie-break 不变，
不能仅凭 clone 出现次数直接实现。

## 12. 下一步：A+ 候选列烘焙（未实现）

用户已明确要求把凛御银灰、戴菲恩、八幡海铃收敛到同一套可选 producer 生命周期，并优先采用“多烘焙、运行时做小型精确 Join”的简单方案。完整交接见 [动态 Producer A+ 联合 Baked Search 计划](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。schema v12 已完成首批贸易 room-local 观察量和完整 row universe 校验，尚不具备完整动态 producer 联合 tuple 与 runtime Join。

schema v12 的 debug CLI 冷进程 `bake validate`（完整贸易 134,324 rows，包含 hash/count、全量
机制签名和 TradePool identity universe 对照）实测约 3.84 秒、峰值 RSS 80,512 KB。这是 catalog
加载/发布门禁成本，不是 warm 查询成本；runtime handle / `OnceLock` 必须只支付一次，后续
signature 查询不得重复反序列化、建池或全量验证。release 冷加载仍未测。

首期方向不是建立新的通用 DP / Pareto 框架，而是：

- 先实现一个语义正确但允许较慢的 live 签名索引 Join，作为 cache miss 的唯一后备；
- Bake 物化紧凑 control 五人行、trade 单房行及少量有限 effect signature；不保存固定 top-K；
- runtime 用 operbox、tier、plan、`used` bitset 过滤；候选只按完全等价 response signature 分桶，桶内保留全部 operator mask，再做 best-first disjoint join；
- response miss 与最终 winner 必须完整写入临时 assignment 后统一 `resolve_base`；精确 Bake hit 可复用离线 solver response，winner 仍只 commit 一次；
- 首期 E2 / 标准 243 快路径不兼容时，回到同语义的 live 签名索引 Join，而不是换一套答案；
- 原始 `C × T₁ × T₂` 笛卡尔积不可执行；DP、Pareto frontier 和 branch-and-bound 只有在签名索引 Join 实测仍不够快时才考虑，不能成为正确性前提。

不能把所有 `LayoutContext` 做笛卡尔积烘焙。A+ 应物化有限候选和已声明的 effect signature，动态上下文仍由真实 resolver/solver 裁决。缓存缺失或失效只能让计算变慢，不能改变候选语义。
