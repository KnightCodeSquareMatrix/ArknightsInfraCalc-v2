---
name: arknights-system-audit
description: 改动或审计涉及体系硬核、producer、同房/跨站/基内 scope、required admission、Team/Shift 绑定、编排、排班或导出时使用。目标是让领域不变量由 System/Rule/Plan/搜索/排班在结构上保证，而不是碰巧被当前排序或房间顺序产出。
---

# 体系与一致性审计

## 不变量从哪来

完整读目标 canonical 文档，只看相关的 registry / data 行。按需覆盖：

- 硬核、档位、最少人数 / 全部可用人数、可选 producer；
- 同房、跨站、基内、配方、容量、互斥；
- Team 成员、Shift 上下班、绑定、闭合与降级；
- 数值 scope、效果原子、用户选择的 policy。

代码、数据、fixture、测试和当前 top hit 只是实现证据，不是规则来源。用户要求严格逐项审计，或 canonical 互相冲突时，完整读 `docs/质量与维护/SYSTEM_AUDIT_WORKFLOW.md` 并遵守其中两次等待。

## 追踪时要分开的几对概念

沿 `select -> plan -> execute -> fill -> resolve -> rotation -> export` 只追到必要处：

- 准入 vs 激活 / 绑定；
- 有资格 vs 必须放置；
- 结算 vs 选型；
- 最终输出 vs 碰巧的样本。

用能改变责任归属的反例去挑战：缺核心、最少成员、竞争候选、房间重排、跨站容量、上下班闭合、policy 变化。违规已经确定在某个 owner 内部时，不需要跑完整个反例清单。

## 排班 / 导出改动的证明

- 用真实 `plan` 或 `layout team-rotation` 入口，写明 mode / profile。
- 逐班核对成员和状态；给一个合法、一个最小非法的绑定 / 休息例子。
- 同一事实在核心轮换、profile / stdout、MAA 三层各核一次；休息班的 producer 要确实停掉。
- 单元测试全绿或 top 排班没变，不算一致性证明。
- `auto_rotation`、`mower_dynamic` 除非明确纳入，否则不在范围内。
