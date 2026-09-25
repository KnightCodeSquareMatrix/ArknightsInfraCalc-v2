# 人间烟火排班链

> 文档角色：system-bridge
> 生命周期状态：current
> 代码范围：crates/infra-core/src/trade/interpreter.rs、schedule/team_rotation.rs、schedule/shift_bind.rs
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：human_fireworks capability、runtime shift bind、办公室绑定补位
> 关联链路：docs/SYSTEM_CHAINS.md、docs/模块/排班与轮换.md
> 核验基线：实际 provider/consumer presence 与 shift bind

机制阈值和倍率引用隔壁知识库；本页只写 solver 如何消费 capability。Plan 先产生实际 provider、consumer 和办公室 producer presence；shift_bind 只绑定实际贡献成员，不能按拥有名单或 tag 预绑定。ABC 中绑定组同上同下，绑定办公室成员从 shared scaffold 排除，休息班再从合法未占候选按 ETA/效率补位。MAA 只导出最终 assignment。

未形成实际消费链时，不写 producer dependency，不占中枢或办公室固定岗位。收益缺失时按 provider presence → consumer presence → resolved_producer_dependencies → shift_bind → final assignment 排查。机制真源变化时先更新知识库桥接，再更新 runtime 映射和本页锚点。
