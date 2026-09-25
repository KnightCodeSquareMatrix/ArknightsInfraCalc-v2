# EffectAtom 机制解释分层

> 文档角色：canonical
> 生命周期状态：current
> 当前真源：`crates/infra-core/src/types.rs`、各域 interpreter 与 `data/skill_table.json`；游戏机制见 [`知识库引用.md`](知识库引用.md)
> 代码范围：`crates/infra-core/src/types.rs`; `skill_table.rs`; `pool/{trade,manufacture,control,power}.rs`; `trade/interpreter.rs`; `manufacture/interpreter.rs`; `control/interpreter.rs`; `cross_facility/interpreter.rs`; `data/skill_table.json`
> 代码锚点：`types.rs::{Phase,Selector,Condition,Action,AtomScope,EffectAtom,CompiledAtom}`; `skill_table.rs::SkillTable::load`; `trade/interpreter.rs::apply_trade_phases`; `manufacture/interpreter.rs::apply_manu_phases`; `control/interpreter.rs::solve_control_with_trace`; `cross_facility/interpreter.rs::orchestrate_global_atoms`
> 关联链路：[`模块/输入与技能映射.md`](模块/输入与技能映射.md)、[`调用链/技能到房间结算.md`](调用链/技能到房间结算.md)、[`模块/中枢与跨设施状态.md`](模块/中枢与跨设施状态.md)
> 核验基线：`947f1a2`

## 设计边界

`EffectAtom` 是运行时解释词汇，不是游戏机制真源。机制事实、技能原文、解锁和同类效果规则由知识库桥接引用；本仓库只定义怎样把已裁决事实映射为输入、状态、phase 和结果。atom 不认识干员名字，owner 只在 trace/诊断中保留。

L1 处理可复用的 selector/action/condition；L2 处理不能安全压平的领域状态（贸易订单、制造产线、发电聚合等）；L3 只对最终实际组合应用具名 shortcut。候选选择、Plan admission、排班和 export 不属于 atom interpreter。

## 数据结构

```rust
EffectAtom {
    atom_id: Option<String>, // JSON `id`，control ledger 要求稳定非空
    selector: Option<Selector>,
    action: Action,
    condition: Option<Condition>,
    tag: Option<String>,
    phase: Phase,
    phase_order: i32,
    scope: AtomScope, // Room（默认）或 Global
}
```

`SkillDef` 以 `id/skill_name/facility/tier/atoms` 存在于 `skill_table.json`。建池时 `CompiledAtom` 附带 `(phase.sort_key, phase_order)`，求解时按序合并。缺失 id、引用或非法 JSON 由 `SkillTable::load`/验证入口失败；空 atoms 可能表示 L2/L3 委托，不能自动当作白板。

## Phase 与常用 action

`Phase::sort_key` 当前顺序为 StateWrite → GlobalInject(由 control 负责) → Constant/PeerShare → Limit → LimitVar/OrderVar → EffVar → PeerAbsorb → OrderMechanic → Mood。phase_order 只在同 phase 内稳定排序。

- `AddFlatEff`、`AddPerGapEff`、selector 派生 action 写入域内 efficiency；
- `AddLimitDelta`、`ReduceLimit` 写入订单/仓库上限；
- `TagOrder` 只给贸易 L2 订单机制提供标签；
- `PeerEffAbsorb` 由对应域执行同房效率转移；
- `StateProduce`/`StateConvert` 写入状态池，`StateConsumeToEff` 在 consumer 域读取；
- `GlobalInject*` 仅由 control ledger/inject manifest 结算；
- `MoodDrain*` 写入 mood drain summary；
- `AddEffRamp` 由域 solver 按班次/平均窗口处理。

新增词汇必须先定义 owner、单位、阶段和失败行为，再补 `types.rs`、runtime JSON 和对应 interpreter；不要在 CLI 或 output 层添加同义字段。

## Room 与 Global

`scope=Room` 是默认值，由 trade/manufacture/power/support interpreter 消费。`scope=Global` 由 `cross_facility::collect_global_atoms` 收集，当前 `orchestrate_global_atoms` 实际执行 StateWrite；control 的 GlobalInject 在 `control/interpreter.rs` 单独执行。per-room 路径必须跳过 Global，避免重复计算。

Global 条件上下文有限：collector 只提供当前 assignment、设施等级、标签和 layout counts；跨设施 interpreter 对不可用 selector/condition 使用明确的 delegated/零贡献策略。需要真实同房条件的 atom 不应伪装成 global。

## 机制扩展流程

1. 从知识库确认原文、解锁、单位和适用设施，并在 [`知识库引用.md`](知识库引用.md) 登记版本/章节。
2. 判断 Room、Global、L2 或 L3 owner，声明 hard constraint 与近似边界。
3. 更新 `types.rs`（若需新词汇）、`skill_table.json`、instances binding 和 interpreter。
4. 若涉及 control/global，补 ledger/inject/resolve；若涉及组合，补 shortcut/segment 与 fixture。
5. 用 `layout eval --explain-json`/域 verify/代表性 fixture 检查 trace、结果和错误传播；高风险变更再跑最终 assignment invariant gate。

## 失败传播

未知 selector/action、缺少 buff、非法单位和不满足 required admission 必须可观察地 skip、delegated、无解或错误；不能静默当零或白板高效。trace 需要保留 owner、atom id、condition 和最终状态，方便从症状沿 [`调用链/技能到房间结算.md`](调用链/技能到房间结算.md) 回溯。
