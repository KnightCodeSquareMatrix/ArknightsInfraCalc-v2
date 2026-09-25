# 内部实现地图

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：对应模块源码；本文只提供导航
> 代码范围：`crates/infra-core/src/{trade,manufacture,cross_facility,global_resource,layout}`
> 代码锚点：见各子文档的代码锚点字段
> 关联链路：`docs/调用链/技能到房间结算.md`
> 核验基线：`947f1a2`

`INTERNAL/` 面向需要直接定位符号的 agent。它描述当前代码路径、输入输出、分支和诊断顺序，不裁决游戏机制；机制原文从 [`../知识库引用.md`](../知识库引用.md) 进入。

| 文档 | 用途 |
|---|---|
| [`TRADE_INTERPRETER.md`](TRADE_INTERPRETER.md) | 贸易 L1 phase、状态字段和 selector |
| [`SHORTCUT_MATCHING.md`](SHORTCUT_MATCHING.md) | shortcut/segment 的实际匹配与互斥 |
| [`TRADE_COMMUNITY_UNIT_OUTPUT.md`](TRADE_COMMUNITY_UNIT_OUTPUT.md) | 单位产出锚、来源和精度边界 |
| [`CROSS_FACILITY.md`](CROSS_FACILITY.md) | global atom 收集、编排和 resource snapshot |
| [`MECHANICS_REGISTRY_AUDIT.md`](MECHANICS_REGISTRY_AUDIT.md) | 生成的 triage 报告，不是覆盖证明 |
