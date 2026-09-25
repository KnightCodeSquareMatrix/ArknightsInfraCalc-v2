# 全基建单班进驻编制

> 文档角色：module-reference
> 生命周期状态：current
> 代码范围：crates/infra-core/src/layout/、crates/infra-core/src/layout/resolve.rs
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：build_plan、execute_plan、assign_shift、BaseAssignment、resolve_base
> 关联链路：docs/ORCHESTRATION_LAYER.md、docs/模块/排班与轮换.md
> 核验基线：BaseAssignment、layout test、layout team-rotation

本文只负责一个时刻的全蓝图岗位状态；多班切换由排班模块负责。输入为 BaseBlueprint、OperBox、operator_instances、skill_table、AssignBaseOptions，输出为 BaseAssignment。一个 assignment 内同一干员不得占多个岗位。

当前流水线为 build_plan → execute_plan → producer/control/dorm claims → resolve_base → trade role/remainder → manufacture 逐房提交并重新 resolve → power → annotate_assignment。producer 先落位，因为它改变 layout.global/global_inject；制造房按稳定顺序提交，不做跨房笛卡尔积全局最优。

岗位高层顺序是 Plan hard anchors、中枢补位、宿舍/全局 producer、发电、贸易、制造、notes。候选在提交前过滤 used；跨房 dependency 只有实际 provider/consumer 都进入 assignment 时成立。机制正文回到知识库。

## 冲突与可诊断字段

每房提交后立即把成员写入 used，下一房基于合并后的 assignment 重算 resolve_base；这使“上一房抢走了当前房候选”成为可复现的顺序事实，而不是缓存随机性。Plan 相关岗位应检查 AssignmentPlan 的 anchors、claims、resolved_producer_dependencies；纯房间效率应检查 RoomEfficiencyLine 和 notes。

单班 assignment 通过 layout test 产生时，不自动拥有跨班 bind 或 dorm policy。将它交给 schedule 前，schedule 会重新建立 profile owner、teams 和 runtime bind；不要把单班 JSON 直接当作轮换报告。
