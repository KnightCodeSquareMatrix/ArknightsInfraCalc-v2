# 排班轮换

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/排班/排班模式.md；docs/排班/定时换班.md
> 摘要：提供排班轮换的短入口、profile 表和完整调用链链接
> 代码范围：crates/infra-core/src/schedule/team_rotation.rs
> 知识库桥接：docs/机制/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：TimedRotationProfile、TeamRotationReport；generic current：schedule_timed_rotation_request
> 关联链路：docs/调用链/定时轮换.md、docs/模块/排班与轮换.md、docs/排班/定时换班.md、docs/排班/泡泡火神源石优先.md
> 核验基线：普通二/三班 topology 请求、TeamShiftResult、assert_final_rotation_invariants

本文是短入口。完整调用链、状态快照、错误传播见定时轮换调用链；模块责任见排班与轮换模块。普通二/三班输入时长序列与plan.compute v7已进入当前实现；契约见ADR 0011与`contracts/plan-compute-v7.md`。当轮CLI已完成fresh release终态验收，范围见[排班模式](排班模式.md)；不据此声称已部署。

下表列具名兼容 preset；普通二/三班还接受按序 topology 时长输入。用户裁决与实现边界见 [ADR 0011](../ADR/0011-generic-two-three-shift-rotation.md)：适用泡泡/火神组合固定归 α 并全局预占，四班路径不变。

| profile | CLI | 班次 | 特例 |
|---|---|---|---|
| abc_12_6_6 | 3、abc、12/6/6、abc-1266 | 12/6/6 | αβγ，γ/α/β 分别休 S1/S2/S3；泡泡/火神首个三级源石房预占，S1 12h，2 上 1 休 |
| abc_12_12_12 | 12/12/12、abc-121212、abc-12-12-12 | 12/12/12 | 同队伍关系，总周期 36h；同源石预占，S1 必含冻结房 |
| main_backup_12_12 | 2、12/12、two-shift、main-backup | 12/12 | 主/替完整状态互斥 |
| main_backup_24_24 | 24/24、24-24、main-backup-2424、main_backup_24_24 | 24/24 | 48h 周期，主/替完整状态互斥 |
| fiammetta_8_8_4_4 | fiammetta-8844、8/8/4/4 | 8/8/4/4 | 菲亚事件 transition |
| abyssal_7_5_7_5 | abyssal-7575、7/5/7/5 | 7/5/7/5 | 深海 producer、五小时 dorm_rest |

**CLI `--rotation` 与 profile 名的区别：**表中第一列的 `abc_12_6_6` 等是 Worker `plan.compute` 的 wire 值，也是 profile/rotation JSON 里的 `rotation_profile` 字段值，不是 CLI 参数。CLI 只接受 CLI 列的写法，或泛型写法 `main-backup@H1/H2`、`abc@H1/H2/H3`（小时数按班次顺序，如 `abc@12/6/6`）；传 `abc_12_6_6` 会报 `unsupported rotation`。泛型写法的时长恰好等于某个具名档位时，会归一化为该具名档位（`abc@12/6/6`→abc_12_6_6，`abc@12/12/12`→abc_12_12_12，`main-backup@12/12`→main_backup_12_12，`main-backup@24/24`→main_backup_24_24），其余时长走泛型路径。裸 `4` 和五班及以上不被接受。

当前普通泛型请求通过 CLI `main-backup@H1/H2`、`abc@H1/H2/H3` 或 Worker v7 topology + `shift_hours` 进入各自 topology 构造，再进入共同完成/终态流程；调用方将总周期限制为 24/36/48h，后端不增加时长业务校验。裸 4、五班及以上不是普通泛型 topology。`shifts` 是最终班次状态；`daily.production` 按真实时长归一到 24h。普通工作房短员是 warning。适用泡泡/火神 pair 的局部预占失败保留普通 peak 并 warning，显式 `--assert-invariants` 仍失败；资格不足或无合法三级房时静默不启用。peak_mood_eta 不是完整周期永续证明。当轮验收范围见排班模式及对应交付，不能冒充本次HEAD新测。

入口为 plan、layout team-rotation 和 serve plan.compute；MAA 输出见 MAA 导出与导入调用链。
