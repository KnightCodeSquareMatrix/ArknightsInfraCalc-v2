# 动态 Producer 旧设计阻塞分析交接

> 文档角色：active-change
> 生命周期状态：ready-on-request
> 当前真源：docs/BASE_ASSIGNMENT.md；docs/CONTROL_CENTER_ASSIGNMENT.md；docs/SCORING_MODEL.md；docs/QUALITY_AND_AUDIT.md
> 摘要：记录回退后的动态 producer 旧模型边界、已验证阻塞和下一位 agent 的只读分析入口

## 1. 当前状态

本工作树已恢复到提交 `1ced9b8` 的动态 producer 路径：

```text
build_plan -> execute_plan -> producer presence candidate
-> system/dorm/power prefix
-> 稳定房间顺序 trade fill
-> 稳定房间顺序 manufacture fill
-> resolve / refresh
-> 比较完整生产前缀
```

该路径是当前生产实现。`TradeManufactureWeightedEfficiencyV1` 已作为关闭旧 raw-inject 剪枝的
shadow 候选接在同一 presence 路径上，但公开默认仍保持 `ControlInjectRawSumV0`。不要把未提交
过的 `control + 全贸易房 tuple` 暴力枚举路径、`joint_prefix.rs` 或 ADR 0004 当作当前能力；
它们已被删除。

## 2. 已修复与已知阻塞

- 已提交 bug 修复停在 `1ced9b8`：无效 operbox progress 被拒绝。
- 此前未提交的 live 联合重构曾完整枚举 control 和贸易房 tuple；full-E2 的首个 control prefix
  有 133,056 个合法 tuple，50 tuple 的联合阶段约 4.15 秒，完整 release 在 180 秒内未完成。
- 该重构虽有小实例 oracle，但绕开了现有分层求解模型，并把每个贸易 tuple 都带入完整制造填房和
  `resolve_base`；因此被回退，不作为后续设计起点。
- 现有旧路径的明确限制是：前一贸易房提交后占用成员，后房才可见该状态；最终 refresh 只能重算
  已选成员，不能恢复前房因跨房 workforce 或人员冲突错过的候选。这是 `policy_restriction`，
  不是已证明的全局最优。
- `plan` / `layout team-rotation --prefix-shadow-out <json>` 会保留公开默认结果并另跑 V1，记录
  房间成员、分域效率、动态 dependency、耗时和失败阶段；shadow 失败不替换默认编制。

## 3. 下一位 agent 的任务

只读分析，不默认实现或恢复联合穷举。回答以下问题：

1. 已报告或可复现的动态 producer 错误 winner，首次发生在机制结算、候选生成、Plan admission、
   顺序提交、上下文刷新还是比较器？没有最小反例时不得假定旧模型一定产生错误结果。
2. 现有模型哪些层可复用：单房完整候选、动态 producer presence、`resolve_base`、response dependency、
   逻辑人员互斥、现有 Bake catalog？
3. 如果确实需要跨房联合选择，最小 affected closure 是什么？先证明它，不要把全制造填房纳入每个
   贸易 tuple 的内层循环。
4. 性能应先分阶段测量：trade row 生成、互斥 join、制造 search、`resolve_base`、snapshot refresh。
   未测量前不得声称制造或 resolve 是唯一瓶颈，也不得预设 Bake 是必要方案。

## 4. 不可违反的边界

- 不使用固定 `top_k`、standalone 缩池、当前 winner 或角色优先级删除动态 producer 合法候选。
- `used`、capacity、Plan required anchor、tier 和禁同房继续是 hard constraint。
- Bake 或 cache miss 只能变慢，不能换到更小候选集合或旧的不同语义路径。
- 任何联合求解必须声明保证等级；没有完整候选和目标证明时只能报告 `BEST_FOUND` 或
  `POLICY_OPTIMAL`，不能称全局最优。
- 不要把临时 cache key 当作领域真源或持久 catalog schema。

## 5. 建议读取顺序

1. `docs/CONTROL_CENTER_ASSIGNMENT.md`
2. `docs/BASE_ASSIGNMENT.md`
3. `docs/SCORING_MODEL.md`
4. `docs/QUALITY_AND_AUDIT.md`
5. `docs/TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md`
6. `docs/TODO/求解器质量与性能优化建议路线图.md`
7. `crates/infra-core/src/layout/assign/pipeline.rs`
8. `crates/infra-core/src/layout/assign/trade_fill.rs`
9. `crates/infra-core/src/layout/assign/manufacture_fill.rs`

## 6. 可复查证据

- 50 tuple 样本：
  `target/codex-runs/dynamic-producer-live-joint/commands/20260730T022812-830525660-1208050-24549/performance-first-prefix-50-cache-sample.log`
- full-E2 180 秒超时：
  `target/codex-runs/dynamic-producer-live-joint/commands/20260730T030310-891239938-1212077-16836/performance-full-e2-release-narrow-hit-list-cache.log`
- 回退前小实例 live-joint 测试：
  `target/codex-runs/dynamic-producer-live-joint/commands/20260730T031356-103037239-1213755-18254/targeted-test-final-live-joint-tests.log`

这些日志只说明已回退方案的行为和成本，不证明当前旧路径有错误，也不授权恢复该方案。
