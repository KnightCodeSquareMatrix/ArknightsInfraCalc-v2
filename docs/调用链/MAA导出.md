# MAA 导出与导入调用链

> 文档角色：call-chain
> 生命周期状态：current
> 当前真源：docs/模块/导出与岗位状态.md
> 机制依赖：docs/知识库引用.md；vault:docs/5-换班操作/定时换班.md
> 摘要：沿最终轮换报告定位 MAA 写出、读回和字段责任
> 代码范围：crates/infra-core/src/export/maa.rs、crates/infra-cli/src/commands/plan_compute.rs
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：`crates/infra-core/src/export/maa.rs::{build_from_team_rotation,build_plan,resolve_drones,load_maa_schedule,assignment_from_maa_plan}`
> 关联链路：docs/调用链/定时轮换.md、docs/模块/导出与岗位状态.md
> 核验基线：MaaSchedule、MaaScheduleImport、plans 与 shifts 数量一致

MAA export 是最终状态的序列化适配器，不重新选择机制、岗位、菲亚目标或无人机房间。

## 写出链

    compute_plan_inner
      └─ requested.maa
           └─ MaaExportOptions::for_blueprint
                └─ build_from_team_rotation
                     ├─ 每个 TeamShiftResult → PlanInput
                     ├─ build_plan
                     │    ├─ build_rooms
                     │    ├─ resolve_scheduled_fiammetta
                     │    ├─ resolve_drones
                     │    └─ dorm autofill
                     └─ wrap_schedule(planTimes)
                          └─ MaaSchedule::save

plan --maa-out 和 serve plan.compute 都走这条路径。save 负责创建父目录、pretty JSON 和写入；失败返回带路径的错误。

## 顶层字段

MaaSchedule 的 author/title/description/id/buildingType 来自 MaaExportOptions。planTimes 显式选项优先，否则由 profile 决定：ABC 为 3班，主替/龙巫为 2班，菲亚插班/深海为 4班。plans 与 TeamRotationReport.shifts 一一对应，顺序不重排。

MaaPlan 包含 name、description、Fiammetta、drones 和 rooms。rooms 使用 MAA 固定名称：trading、manufacture、power、dormitory、control、meeting、hire、processing。每个 MaaRoomSlot 有 skip、product、operators、sort、autofill。

## 菲亚与无人机

若 shift.fiammetta 已有目标，直接投影为 enable/target/order=pre；不重新搜索。没有已确认动作时才使用兼容 priority，不能增加 schedule 次数。

schedule 的 drone_action 只在实际产品为赤金的制造房且 transition 不是 FiammettaOnly 时返回动作；resolve_drones 只把 room_id 映射为蓝图制造房的 1-based index。找不到 id 时 index=1 是输出映射兜底，不是重新选择目标。

## 宿舍与岗位

build_dorm_slots 按最终 assignment 生成宿舍槽位。若轮换 warning 表示普通休息候选不足，build_plan 将宿舍设为 skip=false、autofill=true；这不表示 solver 重新选人。办公室字段名是 hire，会客室为 meeting，加工站为 processing。

## 读回链

    load_maa_schedule(path)
      ├─ read_to_string
      ├─ 去除 UTF-8 BOM
      └─ serde_json::from_str → MaaScheduleImport
           └─ assignment_from_maa_plan
                └─ 未 skip slot → room_id + AssignedOperator
                     └─ elite 从 OperBox 查询，缺省为 0

读回只处理 trading/manufacture/power/dormitory/control，不恢复菲亚、无人机、meeting/hire/processing、profile、班次时长或绑定。读回 assignment 必须由调用方显式 resolve/评估。

## 症状定位

| 症状 | 首查位置 |
|---|---|
| plans 数量不对 | build_from_team_rotation 与 report.shifts |
| 菲亚目标错 | TeamShiftResult.fiammetta、resolve_scheduled_fiammetta |
| 无人机房错 | drone_action 的 RoomProduct、resolve_drones |
| 宿舍被补人 | rotation warnings、build_plan.dorm_autofill |
| 导入练度错 | assignment_from_maa_plan 的 OperBox elite 查询 |
