# 菲亚梅塔换心情规则（导航）

> 文档角色：compatibility-reference
> 生命周期状态：current
> 代码范围：crates/infra-core/src/schedule/team_rotation.rs、crates/infra-core/src/export/maa.rs
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：FiammettaShiftAction、attach_fiammetta_target_actions、resolve_scheduled_fiammetta
> 关联链路：docs/菲亚梅塔使用规则.md、docs/调用链/定时轮换.md
> 核验基线：TeamShiftResult.fiammetta 与 MAA Fiammetta

规则唯一 owner 是菲亚梅塔使用规则。本页只说明责任边界：排班层决定动作，MAA export 序列化动作。fiammetta_enable 是全局开关，不是每班输入。实时心情、Mower 动态换班和跨周期永续不属于当前实现。
