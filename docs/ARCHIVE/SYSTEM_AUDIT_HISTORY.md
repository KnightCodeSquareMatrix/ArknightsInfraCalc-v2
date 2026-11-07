# 体系审计历史摘记

> 文档角色：archive
> 生命周期状态：historical
> 替代项：docs/质量与维护/SYSTEM_AUDIT_WORKFLOW.md
> 历史原因：formal audit 工作流已由 current owner 接管
> 快照日期：2026-07-18
> 摘要：保存系统审计方法和历史阶段记录

> 历史原状态：Archived
> 用途：保留 2026-07-15 前逐项体系审计的阶段性结论与通用案例；不作为当前业务或流程真源。
> 当前规则：业务语义回到对应领域 Markdown，formal audit 流程见 [SYSTEM_AUDIT_WORKFLOW.md](../质量与维护/SYSTEM_AUDIT_WORKFLOW.md)。

## 历史完成项

曾完成或阶段性完成的审计包括红松林、自动化组、迷迭香感知链、莱茵 receiver、贸易核心、控制中枢 / 人间烟火，以及普通制造候选边界。相关实现曾覆盖 resolved `AssignmentPlan`、required anchors、候选最低数量、互斥、跨房 snapshot、实际 dependency 和轮换 bind。

这份列表只表示历史工作发生过，不证明当前实现仍与当时一致，也不替代重新读取当前代码、测试和领域 Markdown。

## 红松林案例

标准全精二结果曾因搜索重新选中成员而掩盖结构错误：制造 fill 只保留同房多个 required anchors 中的首个成员。最终责任边界修正在通用 fill 对全部 recipe anchors 的保留，而不是红松干员或固定房号特判。

该案例形成的长期反例要求是：用最低人数、竞争候选和跨站容量证明 anchor 由结构保留，不能只检查满配总效率。

## 普通制造自然搜索案例

- 软组合应留给最终效率排序，不能因固定 registry 在 solver 前抢占房间和成员。
- standalone 名录、buff 扩展白名单和 baked catalog 都可能隐式裁剪候选池；完整候选范围需要结构标志，而非叠加姓名白名单。
- 候选上下文必须包含候选本身；跨房计数需要按姓名去重并遵守机制上限。
- 后房可能改变前房的跨房状态；需要联合反馈时必须联合枚举，不能只刷新已过时的选择。
- completion 只能消费 resolved plan 事实；显式关闭体系时，system-only 干员不得从普通池回流。
- 自然搜索关系不能升级为 exact `shift_bind`；旧测试若保护错误语义必须改写或删除。

## 当时未关闭风险

历史记录曾提到莱茵支持设施机会成本、自动化第三人完整 solver、迷迭香低心情 alternative、gamma / Recovery 再优化和公开 Rust API source compatibility。它们是历史线索，不是当前 TODO；只有新的用户请求或可复现 bug 才重新进入维护流程。
