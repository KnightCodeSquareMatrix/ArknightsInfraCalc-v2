# 排班轮换

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/模块/排班与轮换.md
> 摘要：提供排班轮换的短入口、profile 表和完整调用链链接
> 代码范围：crates/infra-core/src/schedule/team_rotation.rs
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：TimedRotationProfile、schedule_timed_rotation、TeamRotationReport
> 关联链路：docs/调用链/定时轮换.md、docs/模块/排班与轮换.md、docs/定时换班.md
> 核验基线：六个当前产品 profile、TeamShiftResult、assert_final_rotation_invariants

本文是短入口。完整调用链、状态快照、错误传播见定时轮换调用链；模块责任见排班与轮换模块。

| profile | CLI | 班次 | 特例 |
|---|---|---|---|
| abc_12_6_6 | 3、abc | 12/6/6 | αβγ，γ/α/β 分别休 S1/S2/S3 |
| abc_12_12_12 | abc-121212 | 12/12/12 | 同队伍关系，总周期 36h |
| main_backup_12_12 | 2 | 12/12 | 主/替完整状态互斥 |
| main_backup_24_24 | 24/24、main-backup-2424 | 24/24 | 48h 周期，主/替完整状态互斥 |
| fiammetta_8_8_4_4 | fiammetta-8844 | 8/8/4/4 | 菲亚事件 transition |
| abyssal_7_5_7_5 | abyssal-7575 | 7/5/7/5 | 深海 producer、五小时 dorm_rest |

裸 4、五班及以上和自由时长数组硬错误。shifts 是最终班次状态；daily.production 按真实时长归一到 24h。办公室/会客室不足是 warnings；生产、中枢、绑定和 profile anchor 失败是 errors。peak_mood_eta 不是完整周期永续证明。

入口为 plan、layout team-rotation 和 serve plan.compute；MAA 输出见 MAA 导出与导入调用链。
