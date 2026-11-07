# 贸易站社区等效效率架构设计

> 文档角色：archive
> 生命周期状态：superseded
> 替代项：docs/机制/EFFICIENCY_MODEL.md；docs/机制/SCORING_MODEL.md
> 历史原因：兼容量纲方案已被无兼容直接效率硬切替代
> 快照日期：2026-07-18
> 摘要：保存贸易站社区等效效率架构的被替代设计

> 历史原状态：历史设计稿；当前已完成无兼容硬切。现行架构以
> [../EFFICIENCY_MODEL.md](../../机制/EFFICIENCY_MODEL.md) 为准。下文保留用于追溯最初的贸易域设计，
> 其中“迁移期兼容”与旧字段方案不再适用。
> 日期：2026-07-11
> 范围：贸易站纸面效率、社区等效换算、搜索排序与产出预估
> 需求依据：`排班表图片生成器.md`「三、需求模块二：效率与预期产出计算」
> 关联文档：[../SCORING_MODEL.md](../../机制/SCORING_MODEL.md)、[../SCORING_REFACTOR_PLAN.md](../plans/SCORING_REFACTOR_PLAN.md)、[../EFFECT_ATOM_DESIGN.md](../../机制/EFFECT_ATOM_DESIGN.md)、[../INTERNAL/SHORTCUT_MATCHING.md](../../INTERNAL/SHORTCUT_MATCHING.md)

## 1. 背景

当前贸易站实现同时使用以下字段：

- `order_eff_base`：每名进驻干员提供的 `1%` 人头效率；
- `order_eff_skill`：技能产生的订单效率加成；
- `order_eff_total` / `trade_pct`：纸面效率或 shortcut 覆盖后的等效效率；
- `mechanic_equiv_eff_pct` / `gold_pct`：订单机制解释分量；
- `effective_eff_multiplier`：部分路径将前两个百分比分别转为倍率后相乘；
- `unit_trade_anchor`：社区工具人表提供的单位产出锚点。

这些字段的量纲没有完全统一。以但书为例，当前实现保留纸面 `trade_pct`，另设固定 `gold_pct=55`，再由调试倍率执行 `(1 + trade_pct / 100) × 1.55`。这会造成：

1. 搜索和展示只读取 `trade_pct` 时低估但书；
2. 产出路径可能再次组合 `trade_pct` 与 `gold_pct`；
3. 固定 shortcut 会覆盖实时的人头效率、中枢注入或挂件技能；
4. 字段名无法说明数值是“加成百分比”“总效率百分比”还是“倍率”。

社区已经为但书、可露希尔、龙舌兰、巫恋等特殊贸易机制提供加强后的每日单位产出或等效倍率。本项目不需要还原随机订单、逐笔交付或游戏内部概率，只需要把社区换算作为贸易站的正式结算层。

需求文档进一步明确：最终得分应是一个可以直接参与产出计算的完整效率，而不是还需要调用方补基础 `100%`、中枢、人头或机制倍率的“加成百分比”。

## 2. 目标与非目标

### 2.1 目标

1. 统一贸易效率量纲，任何字段都能从名称判断其单位。
2. 纸面效率只计算一次，并完整包含基础 `100%`、技能、人头效率和中枢注入。
3. 社区单位产出倍率只应用一次，产出、搜索和排班共享同一个最终效率。
4. 但书按“整个房间总效率 × 1.55”动态结算，不再使用固定 `+55%` 分量或固定叙拉古总值。
5. 可露希尔、龙舌兰、巫恋等继续使用社区现成的加强单位产出或分档倍率，不要求还原游戏订单逻辑。
6. 最终搜索得分就是可直接乘统一基准日产出的 `final_efficiency`。
7. 预估产出不再自行拼接基础效率、加成百分比或机制分量。
8. 迁移期间保持 CLI / JSON 可兼容，并给旧字段明确映射和废弃路径。

### 2.2 非目标

- 不模拟订单生成概率、订单队列或逐笔订单。
- 不重新推导社区已经给出的等效效率。
- 不把贸易与制造、发电合成一个综合分。
- 不改 L1 `EffectAtom` 的技能解释职责。
- 不把 `trade_shortcuts.json` 扩展成通用表达式语言。
- 不因本次重构改变体系认领、落位或三队轮换规则。

## 3. 统一效率模型

### 3.1 纸面效率

内部效率统一使用无量纲小数，`1.0` 表示基础 `100%`：

```text
paper_efficiency
  = base_efficiency
  + control_bonus
  + occupancy_bonus
  + operator_skill_bonus
```

| 字段 | 示例 | 含义 |
|------|------|------|
| `base_efficiency` | `1.0` | 制造、贸易站固定基础 `100%` |
| `control_bonus` | `0.07` | 阿米娅等中枢提供 `+7%` |
| `occupancy_bonus` | `0.03` | 三名进驻干员，每人 `+1%` |
| `operator_skill_bonus` | `0.45` | UI 所说“纸面 45%” |
| `paper_efficiency` | `1.55` | 上述分量之和，即纸面总效率 `155%` |

### 3.2 单位产出倍率

需求文档给出两种等价产出公式：

```text
expected_output
  = paper_efficiency
  × shift_hours / 24
  × enhanced_unit_output_per_day
```

或统一折算到三级贸易站基准日产出：

```text
unit_output_multiplier
  = enhanced_unit_output_per_day / reference_unit_output_per_day

final_efficiency
  = paper_efficiency × unit_output_multiplier

expected_output
  = final_efficiency
  × shift_hours / 24
  × reference_unit_output_per_day
```

贸易域使用三级普通贸易站 `10265` 作为 `reference_unit_output_per_day`。普通一级、二级、三级贸易站的 `enhanced_unit_output_per_day` 分别为 `10000`、`10141`、`10265`；特殊机制使用社区给出的加强后单位产出。

`final_efficiency` 是本设计唯一的最终得分：

- 它已经包含基础 `100%`、中枢、人头、技能和特殊机制；
- 它可以直接乘工作时长占比和统一基准日产出；
- 搜索、排班快照和产出预估都读取它；
- 不需要再执行 `1 + pct / 100` 或额外乘 `gold_pct`。

但书简化示例：

```text
paper_efficiency       = 1.23
unit_output_multiplier = 1.55
final_efficiency       = 1.23 × 1.55 = 1.9065
final_efficiency_pct   = 190.65%  // 仅展示
```

### 3.3 等效技能效率是派生展示值

需求文档中的“等效效率”用于让观众把特殊机制与普通技能效率比较。它不是最终得分，也不参与产出公式：

```text
equivalent_operator_skill_bonus
  = final_efficiency
  - (base_efficiency + control_bonus + occupancy_bonus)
```

例如最终效率为 `1.9065`，基础、中枢和人头合计为 `1.10`，则报表可显示等效技能效率 `+80.65%`。该值随中枢和进驻人数变化，必须从最终效率反算，不得作为社区换算的输入。

## 4. 结算流水线

```text
TradeRoomInput
  -> TradeContext + apply_trade_phases
  -> PaperTradeEfficiency
       base_efficiency
       control_bonus
       occupancy_bonus
       operator_skill_bonus
       paper_efficiency
  -> shortcut / segment 只识别社区规则 ID
  -> resolve_trade_production_basis
  -> TradeProductionBasis
       rule_id
       reference_unit_output_per_day
       enhanced_unit_output_per_day
       unit_output_multiplier
  -> FinalTradeEfficiency
       final_efficiency
       equivalent_operator_skill_bonus
  -> TradeResult
  -> search / layout / schedule / CLI / production
```

职责边界：

| 层 | 职责 | 不再负责 |
|----|------|----------|
| L1 interpreter | 计算技能、人头、中枢形成的纸面加成 | 社区等效、产出倍率 |
| L2 order mechanic | 保留需要展示的机制说明和兼容数据 | 生成最终排序分、再次乘倍率 |
| L3 shortcut / segment | 识别组合和选择社区规则 ID | 同时覆盖多个可相乘的效率字段 |
| community conversion | 选择社区加强单位产出并生成单位产出倍率 | 识别干员、执行 EffectAtom |
| final efficiency | `paper_efficiency × unit_output_multiplier` | 再拆成多个参与计算的百分比 |
| production | `基准产出 × final_efficiency × 时长` | 推测 shortcut 该如何组合 |

## 5. 核心类型

建议新增 `crates/infra-core/src/trade/efficiency.rs`：

```rust
pub struct PaperTradeEfficiency {
    pub base_efficiency: f64,
    pub control_bonus: f64,
    pub occupancy_bonus: f64,
    pub operator_skill_bonus: f64,
    pub paper_efficiency: f64,
}

pub struct TradeProductionBasis {
    pub rule_id: Option<String>,
    pub reference_unit_output_per_day: f64,
    pub enhanced_unit_output_per_day: f64,
    pub unit_output_multiplier: f64,
}

pub struct TradeEfficiency {
    pub paper: PaperTradeEfficiency,
    pub production_basis: TradeProductionBasis,
    pub final_efficiency: f64,
    pub equivalent_operator_skill_bonus: f64,
}
```

构造函数维护不变式，调用方不得独立填写可互相推导的字段：

```text
paper_efficiency
  == base_efficiency + control_bonus + occupancy_bonus + operator_skill_bonus

unit_output_multiplier
  == enhanced_unit_output_per_day / reference_unit_output_per_day

final_efficiency
  == paper_efficiency × unit_output_multiplier
```

建议新增 `crates/infra-core/src/trade/equivalent.rs`，只包含社区规则的数据类型和换算函数：

```rust
pub enum CommunityEquivalentRule {
    FacilityBaseline,
    UnitOutputMultiplier { value: f64 },
    EnhancedUnitOutputPerDay { value: f64 },
    EnhancedUnitOutputByLevel { lv1: f64, lv2: f64, lv3: f64 },
}
```

当前已知需求只需要四种具名数据形态：

| 规则 | 用途 |
|------|------|
| `FacilityBaseline` | 普通贸易站，按设施等级选 `10000/10141/10265` |
| `UnitOutputMultiplier` | 已直接给出相对三级普通站的单位产出倍率；但书三级站可用 `1.55` |
| `EnhancedUnitOutputPerDay` | 可露希尔、龙舌兰、巫恋等社区给出加强后的每日单位产出 |
| `EnhancedUnitOutputByLevel` | 但书等机制按设施等级具有不同加强单位产出 |

不提供四则运算表达式、脚本或任意公式 DSL。出现新的社区换算形态时，再根据真实需求新增具名变体。

## 6. 数据结构

`trade_shortcuts.json` 继续承担社区规则与组合锚点的数据真源，但将计算字段收敛为单一 `unit_output`：

```json
{
  "id": "gsl_docus_solo",
  "label": "但书单走",
  "unit_output": {
    "kind": "unit_output_multiplier",
    "value": 1.55
  },
  "match": {
    "kind": "docus"
  }
}
```

社区已给出加强单位产出时直接记录原始值，不先转换成等效加成：

```json
{
  "id": "gsl_witch_long_beta",
  "label": "巫恋+龙舌兰+裁缝β",
  "unit_output": {
    "kind": "enhanced_unit_output_per_day",
    "value": 12739.73
  }
}
```

数据约束：

1. 每个 active shortcut 必须且只能有一个 `unit_output`。
2. `unit_output_multiplier=1.55` 作用于完整 `paper_efficiency`，不能只乘技能加成。
3. `enhanced_unit_output_per_day` 必须注明对应产品和设施等级；不同等级使用 `by_level`。
4. `trade_pct` / `gold_pct` 不再作为计算输入；迁移期只用于旧数据加载或兼容输出。
5. 经社区确认的 `unit_trade_anchor` 应迁移为正式 `unit_output`；未经确认的旧锚点只用于回归参考。
6. 文件级 `source` 保留；若同一文件混入不同来源，条目增加 `source`，但不引入额外置信度系统。

## 7. 但书与叙拉古链

`gsl_docus_solo` 和 `gsl_docus_syracusa` 使用同一套但书单位产出规则。二者可以保留不同 ID，用于体系识别、调试与回归，但不得拥有不同的效率公式。

叙拉古链仍由以下条件决定是否命中：

- 贸易站为但书 + 伺夜 + 贝洛内；
- 中枢有精二八幡海铃；
- 不违反但书同房互斥规则。

八幡海铃、阿米娅或其他中枢产生的实际贸易注入先进入 `paper_efficiency`，随后整体乘但书的单位产出倍率。三级站简化为 `paper_efficiency × 1.55`；一级、二级站优先读取社区给出的分级加强日产出，再相对 `10265` 转成倍率，不得直接复用三级 `1.55`。`gsl_docus_syracusa` 中现有固定 `trade_pct=200` 只作为旧锚点迁移参考，不能继续覆盖运行时结果。

## 8. 搜索、排班与产出

### 8.1 搜索排序

贸易搜索统一按 `final_efficiency` 排序：

```text
TradeSearchHit.score = final_efficiency
```

`score=1.9065` 表示相对三级普通贸易站基准产出的直接 `190.65%` 效率。breakdown 同时提供纸面效率、单位产出倍率和派生等效技能效率。

兼容字段 `trade_pct` 若必须保留，迁移期定义为 `final_efficiency × 100`，即完整效率百分比；不得再沿用“基础 `100%` 之上的加成”语义。

### 8.2 排班快照

`RoomEfficiencySnapshot` 最终应保存：

- `trade_paper_efficiency`；
- `trade_unit_output_multiplier`；
- `trade_final_efficiency`；
- `trade_equivalent_operator_skill_bonus`；
- `trade_rule_id`。

旧 `trade_score` / `trade_pct` / `trade_gold_pct` 在兼容期继续序列化，但只从新结构派生，不再作为真源。

### 8.3 产出预估

产出入口只接收最终倍率：

```text
estimated_output
  = reference_unit_output_per_day
  × final_efficiency
  × shift_hours / 24
```

`daily_yield` 不再接收名称含糊的 `order_eff_total_pct`。建议参数改为 `final_efficiency`，从类型和调用点阻止 `/100` 与 `1 + pct/100` 混用。

社区贸易等效效率只能直接推导其定义覆盖的产出分量。如果 `gold_pct` 表示赤金需求解释而不是贸易产出倍率，则继续单独展示，不能乘进 `estimated_output`。赤金消耗预估只有在社区给出独立可靠换算时才接入。

## 9. 兼容迁移

### Phase 1：建立新类型，不改结果

- 新增 `PaperTradeEfficiency` / `TradeProductionBasis` / `TradeEfficiency`。
- 普通站先使用 `FacilityBaseline`。
- `TradeResult` 增加 `efficiency`，旧字段从新结构派生。
- 给百分比与倍率不变式补单元测试。

### Phase 2：迁移社区规则

- `trade_shortcuts.json` 增加 `unit_output`。
- 但书 solo 与叙拉古链改为纸面效率乘社区单位产出倍率。
- 可露希尔、龙舌兰、巫恋逐条把社区加强单位产出迁入正式数据。
- shortcut 匹配逻辑保持不变，只改变匹配结果携带的数据。

### Phase 3：切换消费者

- `search/trade.rs` 按 `final_efficiency` 排序。
- `box_profile`、`bake`、layout 快照、排班汇总读取新结构。
- CLI / JSON 同时输出纸面效率、单位产出倍率、最终效率和规则 ID。
- `daily_yield` 改为消费 `final_efficiency`。

### Phase 4：删除双重口径

- 删除 `TradeShortcutEntry.trade_pct` / `gold_pct` 的计算职责。
- 删除 `TradeShortcutMatch::effective_multiplier()` 的双百分比相乘实现。
- `effective_eff_multiplier`、`mechanic_equiv_eff_pct`、`trade_gold_pct` 完成废弃或降级为纯解释字段。
- 重新 bake 贸易候选数据，禁止混用旧缓存。

## 10. 回归与验收

### 10.1 数学不变式

- 无技能、无中枢、无人头时 `paper_efficiency=1.0`。
- 纸面加成共 `23%` 时 `paper_efficiency=1.23`。
- `final_efficiency == paper_efficiency × unit_output_multiplier`。
- 普通三级站的 `unit_output_multiplier=1.0`。
- 所有产出路径只消费一次 `final_efficiency`。

### 10.2 但书回归

| 纸面加成 | 纸面效率 | 单位产出倍率 | 最终效率 |
|----------|----------|--------------|----------|
| `23%` | `1.23` | `1.55` | `1.9065` |
| `30%` | `1.30` | `1.55` | `2.015` |
| `83%` | `1.83` | `1.55` | `2.8365` |

另需断言：

- 人头从 2 人变 3 人时，增加的 `1%` 也进入 `×1.55`；
- 中枢注入 `+7%` 位于乘法之前；
- solo 与叙拉古链在相同纸面值和设施等级下得到相同最终效率；
- 叙拉古链不再固定返回 `200%`；
- `gold_pct=55` 不得再次参与产出或排序。

直接产出断言：三级但书站在 `paper_efficiency=1.23`、工作 `12h` 时，预计龙门币产出为：

```text
10265 × 1.9065 × 12 / 24 = 9785.11125
```

搜索得分必须为 `1.9065`，不得写成 `90.65`、`190.65` 或 `1.23`；百分比只在 UI 层格式化为 `190.65%`。

### 10.3 固定社区锚点回归

- 可露希尔、龙舌兰、巫恋当前各分档的加强单位产出与社区表一致；
- 固定规则不受未定义的额外乘法影响；
- shortcut 识别结果和同房互斥保持不变。

### 10.4 集成验收

```bash
mkdir -p target/codex-logs
cargo test -p infra-core --no-run > target/codex-logs/infra-core-test-build.log 2>&1
tail -80 target/codex-logs/infra-core-test-build.log
cargo test -p infra-core --quiet
cargo run -q -p infra-cli -- verify --all
cargo run -q -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text
```

验收结果必须同时检查：

1. 但书站的纸面效率、单位产出倍率、规则 ID、最终效率；
2. 贸易搜索排序是否直接使用最终效率；
3. `plan` / `team-rotation` 是否仍选择预期贸易核心；
4. JSON 中旧字段与新字段是否满足映射关系；
5. 产出是否只应用一次 `final_efficiency`。

## 11. 最终决策摘要

1. L1 继续计算基础、技能、人头和中枢形成的完整纸面效率。
2. L3 只识别社区规则，不再同时提供多个参与计算的百分比。
3. 社区规则提供加强单位产出或相对统一基准的单位产出倍率。
4. `final_efficiency = paper_efficiency × unit_output_multiplier`，它是唯一最终得分。
5. 但书三级站简化为完整纸面效率乘 `1.55`；分级数据按社区加强单位产出折算。
6. 可露希尔、龙舌兰、巫恋使用社区给定的加强单位产出或分档倍率。
7. 搜索、排班和产出全部消费 `final_efficiency`。
8. 等效技能效率只为报表展示，从最终效率反算。
9. `gold_pct` 等旧解释分量不得参与隐式二次乘法。
10. 本设计不引入真实订单模拟。
