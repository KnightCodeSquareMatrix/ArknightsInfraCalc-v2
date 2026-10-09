# 全基建单班进驻编制

> 文档角色：module-reference
> 生命周期状态：current
> 当前真源：docs/ORCHESTRATION_LAYER.md
> 摘要：说明单班 BaseAssignment 的 Plan、落位、互斥和 resolve 责任
> 代码范围：crates/infra-core/src/layout/、crates/infra-core/src/layout/resolve.rs
> 知识库桥接：docs/知识库引用.md（机制正文与版本由隔壁知识库裁决）
> 代码锚点：build_plan、execute_plan、assign_shift、BaseAssignment、resolve_base
> 关联链路：docs/ORCHESTRATION_LAYER.md、docs/模块/排班与轮换.md
> 核验基线：BaseAssignment、layout test、layout team-rotation

本文只负责一个时刻的全蓝图岗位状态；多班切换由排班模块负责。输入为 BaseBlueprint、OperBox、operator_instances、skill_table、AssignBaseOptions，输出为 BaseAssignment。一个 assignment 内同一干员不得占多个实际位置；`required`、pinned 或为满足技能 presence 而放入岗位的支持者不是特殊干员身份，不能绕过唯一占用、房间容量和设施/配方/订单匹配。跨周期休班规则由[定时换班](定时换班.md#每周期休班底线目标规则)裁决。

当前流水线为 build_plan → execute_plan → producer/control/dorm claims → resolve_base → trade role/remainder → manufacture 逐房提交并重新 resolve → power → annotate_assignment。producer 先落位，因为它改变 layout.global/global_inject；制造房按稳定顺序提交，不做跨房笛卡尔积全局最优。

岗位高层顺序是 Plan hard anchors、中枢补位、宿舍/全局 producer、发电、贸易、制造、notes。候选在提交前过滤 used；跨房 dependency 只有实际 provider/consumer 都进入 assignment 时成立。机制正文回到知识库。

会客室在生产选位之后从空闲池比较双人房间的真实线索速度。精二忍冬与铃兰是可比较的搭档候选，铃兰不单独进入普通会客候选；其他组合收益更高时不选搭档。Plan 已确认的岗位不受普通候选过滤。

## 冲突与可诊断字段

每房提交后立即把成员写入 used，下一房基于合并后的 assignment 重算 resolve_base；这使“上一房抢走了当前房候选”成为可复现的顺序事实，而不是缓存随机性。Plan 相关岗位应检查 AssignmentPlan 的 anchors、claims、resolved_producer_dependencies；纯房间效率应检查 RoomEfficiencyLine 和 notes。

当前 `BaseAssignment::operator_names()` 只从 `rooms` 收集成员；`training_assist` 是单独字段。现有房间 `used` 过滤和轮换的房间成员唯一性检查不等于已经统一校验训练协助位与房间成员的互斥；该字段的跨位置唯一占用仍需补齐，不能将上述业务约束写成已证明的运行时保证。

单班 assignment 通过 layout test 产生时，不自动拥有跨班 bind 或 dorm policy。将它交给 schedule 前，schedule 会重新建立 profile owner、teams 和 runtime bind；不要把单班 JSON 直接当作轮换报告。
