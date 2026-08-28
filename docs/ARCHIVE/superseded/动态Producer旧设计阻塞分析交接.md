# 动态 Producer 旧设计阻塞分析交接

> 文档角色：archive
> 生命周期状态：superseded
> 替代项：docs/TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md；docs/CONTROL_CENTER_ASSIGNMENT.md
> 历史原因：联合穷举方案已回退，当前动态 producer 设计由现行 canonical 和 A+ active-change 承接
> 摘要：保留已回退动态 producer 联合搜索方案的性能证据、限制和保证边界

## 历史结论

- 当前生产主路径是 `build_plan -> execute_plan -> producer presence -> prefix -> trade/manufacture fill -> resolve`。
- 曾尝试完整枚举 control 与贸易房 tuple；full-E2 首个 control prefix 有 133,056 个合法 tuple，50 tuple 联合阶段约 4.15 秒，完整 release 在 180 秒内未完成。
- 该方案把完整制造填房和 `resolve_base` 放入贸易 tuple 内层循环，绕过现有分层求解模型，因此已回退，不是当前能力或后续默认起点。
- 现行逐房提交是 `policy_restriction`，不能据此声称全局最优；后续仅在出现最小错误 winner 或明确产品需求时恢复 affected closure 调查。

## 不变量与证据边界

- `used`、capacity、Plan required anchor、tier 和禁同房仍是 hard constraint。
- Bake/cache miss 只能变慢，不能改变合法候选集合或语义路径。
- 没有完整候选与目标证明时只能报告 `BEST_FOUND` 或 `POLICY_OPTIMAL`，不能称全局最优。
- 历史日志只证明已回退方案的行为和成本，不证明当前旧路径有错误。

## 证据入口

- `target/codex-runs/dynamic-producer-live-joint/commands/20260730T022812-830525660-1208050-24549/performance-first-prefix-50-cache-sample.log`
- `target/codex-runs/dynamic-producer-live-joint/commands/20260730T030310-891239938-1212077-16836/performance-full-e2-release-narrow-hit-list-cache.log`
- `target/codex-runs/dynamic-producer-live-joint/commands/20260730T031356-103037239-1213755-18254/targeted-test-final-live-joint-tests.log`
