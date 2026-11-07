# MAA 导出与导入调用链

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/模块/导出与岗位状态.md
> 机制依赖：docs/机制/知识库引用.md；vault:docs/5-换班操作/定时换班.md
> 摘要：沿最终轮换报告定位 MAA 写出、读回和字段责任
> 代码范围：crates/infra-core/src/export/maa.rs、crates/infra-cli/src/commands/plan_compute.rs
> 知识库桥接：docs/机制/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：`crates/infra-core/src/export/maa.rs::{build_from_team_rotation,build_plan,resolve_drones,load_maa_schedule,assignment_from_maa_plan}`
> 关联链路：docs/调用链/定时轮换.md、docs/模块/导出与岗位状态.md
> 核验基线：菲亚 24/24 Worker plan/plan.compute 与 MAA 投影证据见 `target/codex-runs/fiammetta-2424-20261020-r7/manifest.json`

MAA export 是最终状态的序列化适配器，不重新选择机制、岗位、菲亚目标或无人机房间。

## 当前字段状态

以下是当前源码事实，不是回收站 proposal 的目标状态：

| 字段/分组 | 当前实现 | 说明 |
|---|---|---|
| `rooms.trading`、`manufacture`、`power`、`dormitory`、`control`、`meeting`、`hire`、`processing` | 已投影 | `build_rooms` 逐类从最终 `BaseAssignment` 读取成员；export 不重新选人。 |
| `rooms.recycle` | 已投影 | 每个蓝图回收站写一个 `MaaRecycleSlot`，字段仅为 `operators`，成员直接来自该房间的最终 `BaseAssignment`。 |
| `operatorsA` / `operatorsB` | 不在本次输出范围 | MAA 的 A/B 是训练室字段；回收站遵循普通 `operators` 名单，不沿用旧 A/B 草案。 |

官方 [MAA 基建排班协议示例](https://github.com/MaaAssistantArknights/MaaAssistantArknights/blob/5dd0ea8aac66921f6a307359bca3a84bdfcc9441/docs/zh-cn/protocol/base-scheduling-schema.md#L153-L156) 中，`rooms.recycle` 是与制造、贸易同级的数组，示例项为 `{ "operators": ["芬", "克洛丝"] }`；固定版本 [schema](https://github.com/MaaAssistantArknights/MaaAssistantArknights/blob/5dd0ea8aac66921f6a307359bca3a84bdfcc9441/docs/maa-infra.schema.json#L189-L195) 将其限制为最多一个普通房间项。该协议仅证明所查上游提交的字段，不代表所有 MAA 发布版 GUI 均已启用；export 只投影、不补选。回收站轮换设计及新旧协议差异见 [回收站设计稿 §7](../TODO/详情/核心求解/回收站选人放人与轮换设计.md#7-后续-plancompute-v8-契约)。

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

本次 WIP 的 named rotation 与 `rooms.recycle` 投影为已提交行为；generic 二/三班的实际时长、planTimes 和 v7 请求也已提交，但尚未 fresh release 验收，不能据此保证所有部署版本或 MAA 执行端支持。

## 顶层字段

MaaSchedule 的 author/title/description/id/buildingType 来自 MaaExportOptions。planTimes 显式选项优先，否则由兼容 profile 策略串决定：ABC/generic ABC 为 3班，主备/generic main-backup 为 2班，菲亚插班/深海为 4班。泛型每计划名称、描述与下次执行间隔使用最终实际时长。二班休班队宿舍 autofill 使用显式 MAA 策略事实，不以 `plan_times()=="2班"` 充当策略 owner。独立龙巫二班已删除；旧请求在导出前失败，不会映射到主备。plans 与 TeamRotationReport.shifts 一一对应，顺序不重排。

MaaPlan 包含 name、description、Fiammetta、drones 和 rooms。rooms 使用 MAA 固定名称：trading、manufacture、power、dormitory、control、meeting、hire、processing、recycle。通用 `MaaRoomSlot` 承载已有房间字段；回收站用专属 `MaaRecycleSlot`，仅序列化 `operators`，没有 A/B、product 或额外补位行为。布局不含回收站时 `recycle` 省略；含回收站时按蓝图顺序写出其最终名单。

## 菲亚与无人机

若 shift.fiammetta 已有目标，直接投影为 enable/target/order=pre；不重新搜索。24/24 的该计划 description 说明本班实际菲亚目标，不能据此推导目标也在另一班工作。定时轮换传入空 priority，没有已确认动作时保持关闭，不会在导出层补回被 ready 排除的动作。兼容 priority 字段不参与这条定时轮换主路径。

schedule 的 drone_action 只在实际产品为赤金的制造房且 transition 不是 FiammettaOnly 时返回动作；resolve_drones 只把 room_id 映射为蓝图制造房的 1-based index。找不到 id 时 index=1 是输出映射兜底，不是重新选择目标。

## 宿舍与岗位

build_dorm_slots 按最终 assignment 生成宿舍槽位。普通休息短缺的 autofill 是宿舍完成提交的内部执行事实；二班休班队的既有 autofill policy 继续生效。export 不读取 warning 文案决定执行方式，不重新选人。办公室字段名为 hire，会客室为 meeting，加工站为 processing。

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
| 宿舍被补人 | 最终 assignment 的 dorm_autofill、二班休班队 policy、build_plan |
| 导入练度错 | assignment_from_maa_plan 的 OperBox elite 查询 |
