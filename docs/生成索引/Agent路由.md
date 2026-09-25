# Agent 路由（生成）

> 文档角色：generated-reference
> 生命周期状态：generated
> 当前真源：docs/agent-index/README.md
> 生成器：scripts/codex/generate_agent_index.py
> 摘要：由稳定 route 注册表生成的症状、入口、调用链、模块、机制和验证导航

<!-- GENERATED: do not edit -->
## `CLI.PLAN`：完整 plan 请求

**优先级**：`65`

**症状**：plan 失败、profile 不对、MAA 与文本不一致、完整方案

**判别**：请求入口、默认参数、Worker/CLI 汇合或 profile/MAA 同源问题时选择。

**入口**：`crates/infra-cli/src/main.rs::run`；`crates/infra-cli/src/commands/plan_compute.rs::compute_plan`

**完整调用链**：[docs/调用链/plan请求.md](../调用链/plan请求.md)

**模块 owner**：[docs/接口/CLI与Worker.md](../接口/CLI与Worker.md)；[docs/模块/排班与轮换.md](../模块/排班与轮换.md)

**机制引用**：[docs/知识库引用.md#solver-文档的引用格式](../知识库引用.md#solver-文档的引用格式)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/4-排班算法/可露希尔基建终端排班机制.md`；`docs/5-换班操作/定时换班.md`

**验证**：`infra-cli plan --operbox <path> --profile-out out/<task>-profile.json --maa-out out/<task>-maa.json`

## `LAYOUT.SHIFT`：单班布局与设施填充

**优先级**：`75`

**症状**：干员被抢、体系未激活、房间没填满、候选池异常

**判别**：单班 assignment、used/capacity、体系准入或干员冲突异常时选择。

**入口**：`crates/infra-core/src/layout/assign.rs::assign_shift_with_plan_skip`；`crates/infra-core/src/layout/orchestrate/select.rs::build_plan`

**完整调用链**：[docs/调用链/跨设施状态刷新.md](../调用链/跨设施状态刷新.md)

**模块 owner**：[docs/模块/候选搜索与Bake.md](../模块/候选搜索与Bake.md)；[docs/模块/中枢与跨设施状态.md](../模块/中枢与跨设施状态.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/设施与进驻/基建总览.md`；`docs/3-组合库/组合注册表.md`

**验证**：`infra-cli layout test --layout <path> --operbox <path>`

## `SOLVER.TRADE_ROOM`：贸易站单房结算

**优先级**：`90`

**症状**：贸易效率错误、订单类型错误、shortcut 未命中、单位产出异常

**判别**：只有固定贸易站实际组合的订单、shortcut 或单位产出异常时选择。

**入口**：`crates/infra-core/src/trade/solver.rs::solve_trade_with_shift`；`crates/infra-core/src/trade/interpreter.rs`

**完整调用链**：[docs/调用链/技能到房间结算.md](../调用链/技能到房间结算.md)

**模块 owner**：[docs/模块/贸易求解.md](../模块/贸易求解.md)；[docs/模块/输入与技能映射.md](../模块/输入与技能映射.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/设施与进驻/贸易站机制.md`；`docs/2-干员技能/技能-贸易站.md`；`docs/2-干员技能/释义/特别订单.md`

**验证**：`infra-cli verify --all`；`infra-cli trade yield <fixture>`

## `ROTATION.TIMED`：定时轮换

**优先级**：`70`

**症状**：班次互斥错误、同上同下错误、profile 不识别、休息状态错误

**判别**：profile、班次互斥、shift_bind、连续工作或 dorm_rest 异常时选择。

**入口**：`crates/infra-core/src/schedule/team_rotation.rs::schedule_timed_rotation`

**完整调用链**：[docs/调用链/定时轮换.md](../调用链/定时轮换.md)

**模块 owner**：[docs/模块/排班与轮换.md](../模块/排班与轮换.md)；[docs/模块/导出与岗位状态.md](../模块/导出与岗位状态.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/5-换班操作/定时换班.md`；`docs/1-基础设定/设施与进驻/心情与宿舍.md`

**验证**：`infra-cli layout team-rotation --layout <path> --operbox <path> --json`

## `EXPORT.MAA`：MAA 导出

**优先级**：`60`

**症状**：MAA 字段错误、导出房间与 assignment 不同、轮换数量错误

**判别**：只有最终 rotation 正确而 MAA 字段/房间/数量错误时选择。

**入口**：`crates/infra-core/src/export/maa.rs::build_from_team_rotation`

**完整调用链**：[docs/调用链/MAA导出.md](../调用链/MAA导出.md)

**模块 owner**：[docs/模块/导出与岗位状态.md](../模块/导出与岗位状态.md)；[docs/接口/输入输出与兼容.md](../接口/输入输出与兼容.md)

**机制引用**：[docs/知识库引用.md#唯一边界](../知识库引用.md#唯一边界)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/5-换班操作/定时换班.md`

**验证**：`infra-cli plan --operbox <path> --maa-out out/<task>-maa.json`

## `DIAGNOSTICS.DEBUG`：debug bundle 重放

**优先级**：`55`

**症状**：线上反馈复现、求解错误、某房间异常、某干员未进池

**判别**：需要 debug bundle 重放、explain-json 或错误阶段定位时选择。

**入口**：`crates/infra-cli/src/commands/debug.rs::debug_cmd`

**完整调用链**：[docs/调用链/debug与验证.md](../调用链/debug与验证.md)

**模块 owner**：[docs/接口/错误与诊断.md](../接口/错误与诊断.md)；[docs/模块/候选搜索与Bake.md](../模块/候选搜索与Bake.md)

**机制引用**：[docs/知识库引用.md#不可混淆的四种内容](../知识库引用.md#不可混淆的四种内容)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/README.md`

**验证**：`infra-cli debug rerun --bundle <debug-bundle.json>`

## `SOLVER.MANUFACTURE_ROOM`：制造站单房结算

**优先级**：`90`

**症状**：制造效率错误、仓库上限异常、配方或产线无解

**判别**：只有固定制造站配方、产线、仓储或制造效率异常时选择。

**入口**：`crates/infra-core/src/manufacture/interpreter.rs::apply_manu_phases`；`crates/infra-core/src/manufacture/solver.rs::solve_manufacture_with_shift`

**完整调用链**：[docs/调用链/技能到房间结算.md](../调用链/技能到房间结算.md)

**模块 owner**：[docs/模块/制造求解.md](../模块/制造求解.md)；[docs/模块/输入与技能映射.md](../模块/输入与技能映射.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/设施与进驻/制造站机制.md`；`docs/2-干员技能/技能-制造站.md`；`docs/1-基础设定/资源体系/产出常数表.md`

**验证**：`infra-cli layout eval --explain-json <args>`；`infra-cli plan --assert-invariants`

## `SOLVER.POWER_ROOM`：发电与无人机结算

**优先级**：`90`

**症状**：发电效率错误、无人机库存或动作异常、电站人数不对

**判别**：只有发电站效率、power counts 或 DroneAction 异常时选择。

**入口**：`crates/infra-core/src/power/interpreter.rs::apply_power_phases`；`crates/infra-core/src/power/solver.rs::solve_power`

**完整调用链**：[docs/调用链/技能到房间结算.md](../调用链/技能到房间结算.md)

**模块 owner**：[docs/模块/心情与辅助设施.md](../模块/心情与辅助设施.md)；[docs/模块/导出与岗位状态.md](../模块/导出与岗位状态.md)；[docs/DRONES.md](../DRONES.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/设施与进驻/发电站机制.md`；`docs/2-干员技能/技能-发电站.md`；`docs/1-基础设定/设施与进驻/心情与宿舍.md`

**验证**：`infra-cli layout eval --explain-json <args>`；`infra-cli plan --assert-invariants`

## `CONTROL.CROSS_FACILITY`：中枢与跨设施状态

**优先级**：`95`

**症状**：中枢效果不生效、producer/consumer 未联动、跨房间注入过期

**判别**：当症状包含中枢、producer、consumer、global manifest 或跨房间注入时选择。

**入口**：`crates/infra-core/src/control/ledger.rs::ControlEffectLedger`；`crates/infra-core/src/cross_facility/interpreter.rs::orchestrate_global_atoms`；`crates/infra-core/src/layout/resolve.rs::resolve_base`

**完整调用链**：[docs/调用链/跨设施状态刷新.md](../调用链/跨设施状态刷新.md)

**模块 owner**：[docs/模块/中枢与跨设施状态.md](../模块/中枢与跨设施状态.md)；[docs/CONTROL_CENTER_ASSIGNMENT.md](../CONTROL_CENTER_ASSIGNMENT.md)；[docs/INTERNAL/CROSS_FACILITY.md](../INTERNAL/CROSS_FACILITY.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/设施与进驻/控制中枢机制.md`；`docs/3-组合库/组合注册表.md`；`docs/3-组合库/跨设施体系/自动化组.md`

**验证**：`infra-cli layout eval --explain-json <args>`；`infra-cli plan --assert-invariants`

## `INPUT.SKILL_MAPPING`：输入、精英化与技能映射

**优先级**：`100`

**症状**：干员未进候选池、tier 或设施技能不对、buff_id 缺失

**判别**：当问题涉及 tier、instances、buff_id 或候选池缺失时优先选择。

**入口**：`crates/infra-core/src/tier.rs::PromotionTier`；`crates/infra-core/src/instances.rs::resolve_buff_ids`；`crates/infra-core/src/skill_table.rs::SkillTable::load`

**完整调用链**：[docs/调用链/技能到房间结算.md](../调用链/技能到房间结算.md)

**模块 owner**：[docs/模块/输入与技能映射.md](../模块/输入与技能映射.md)；[docs/MODELLED_OPERATORS.md](../MODELLED_OPERATORS.md)；[docs/EFFECT_ATOM_DESIGN.md](../EFFECT_ATOM_DESIGN.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/2-干员技能/README.md`；`docs/2-干员技能/名册.md`；`docs/1-基础设定/设施与进驻/基建总览.md`

**验证**：`infra-cli layout eval --explain-json <args>`；`python3 scripts/audit_mechanics_registry.py`

## `SEARCH.BAKE`：候选搜索与 Bake

**优先级**：`85`

**症状**：候选没被考虑、Bake 命中结果旧、top_k 被误认为全局最优

**判别**：候选未入池、排序/剪枝或 Bake catalog 异常时选择；若已选组合结算错误转到对应 SOLVER。

**入口**：`crates/infra-core/src/pool/base.rs::filter_pool`；`crates/infra-core/src/search/trade.rs::search_trade_triples`；`crates/infra-core/src/bake.rs::validate_baked_catalog`

**完整调用链**：[docs/调用链/候选搜索与Bake.md](../调用链/候选搜索与Bake.md)

**模块 owner**：[docs/模块/候选搜索与Bake.md](../模块/候选搜索与Bake.md)；[docs/SCORING_MODEL.md](../SCORING_MODEL.md)；[docs/PERFORMANCE_ENGINEERING.md](../PERFORMANCE_ENGINEERING.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/设施与进驻/基建总览.md`；`docs/3-组合库/组合注册表.md`

**验证**：`infra-cli bake validate <args>`；`infra-cli bake verify <args>`；`infra-cli plan --assert-invariants`

## `MOOD.SUPPORT`：心情与辅助设施

**优先级**：`80`

**症状**：ETA 不足、办公室/会客室补位异常、宿舍恢复或 warning 错误

**判别**：ETA、宿舍、办公室、会客室或辅助设施状态异常时选择。

**入口**：`crates/infra-core/src/mood/eta.rs::shift_eta_with_instances`；`crates/infra-core/src/support_facility.rs::evaluate_support_room`；`crates/infra-core/src/layout/resolve.rs::resolve_base`

**完整调用链**：[docs/调用链/跨设施状态刷新.md](../调用链/跨设施状态刷新.md)

**模块 owner**：[docs/模块/心情与辅助设施.md](../模块/心情与辅助设施.md)；[docs/模块/排班与轮换.md](../模块/排班与轮换.md)；[docs/DRONES.md](../DRONES.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/设施与进驻/心情与宿舍.md`；`docs/1-基础设定/设施与进驻/办公室机制.md`；`docs/1-基础设定/设施与进驻/会客室机制.md`

**验证**：`infra-cli layout eval --explain-json <args>`；`infra-cli layout team-rotation --assert-invariants <args>`

## `VERIFY.REGRESSION`：回归验证与失败证据

**优先级**：`45`

**症状**：回归 case 失败、线上 bundle 需要复现、错误阶段不清楚

**判别**：需要运行既有 regression case 或解释固定 assignment 时选择。

**入口**：`crates/infra-cli/src/commands/verify.rs::verify_cmd`；`crates/infra-cli/src/commands/debug.rs::rerun_cmd`

**完整调用链**：[docs/调用链/debug与验证.md](../调用链/debug与验证.md)

**模块 owner**：[docs/接口/错误与诊断.md](../接口/错误与诊断.md)；[docs/求解错误分析报告.md](../求解错误分析报告.md)；[docs/模块/候选搜索与Bake.md](../模块/候选搜索与Bake.md)

**机制引用**：[docs/知识库引用.md#不可混淆的四种内容](../知识库引用.md#不可混淆的四种内容)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/README.md`

**验证**：`infra-cli verify --all`；`infra-cli debug rerun --bundle <debug-bundle.json>`

## `ADVICE.TRAINING`：排班附带的练卡建议

**优先级**：`50`

**症状**：training_advice 不一致、练卡建议缺少组合说明、建议与当前轮换脱节

**判别**：只有 training_advice 输出或组合说明与当前 rotation 不一致时选择。

**入口**：`crates/infra-cli/src/commands/serve.rs::build_plan_compute_training_advice`；`crates/infra-cli/src/commands/plan_compute.rs::compute_plan_for_plan_compute`

**完整调用链**：[docs/调用链/plan请求.md](../调用链/plan请求.md)

**模块 owner**：[docs/练卡推荐相关/README.md](../练卡推荐相关/README.md)；[docs/练卡推荐相关/玩家输出口径.md](../练卡推荐相关/玩家输出口径.md)；[docs/接口/输入输出与兼容.md](../接口/输入输出与兼容.md)

**机制引用**：[docs/知识库引用.md#可引用范围](../知识库引用.md#可引用范围)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/2-干员技能/README.md`；`docs/3-组合库/组合总览.md`

**验证**：`infra-cli plan --operbox <path> --maa-out out/<task>-maa.json`；`infra-cli serve`

## `WORKFLOW.FAILURE_EVIDENCE`：失败台账与反馈复现

**优先级**：`40`

**症状**：用户反馈无解、失败族需要按指纹检索、证据包缺少首个失败阶段

**判别**：需要记录失败证据、按 solver 指纹检索或复现线上 bundle 时选择。

**入口**：`crates/infra-cli/src/commands/debug.rs::rerun_cmd`；`crates/infra-core/src/schedule/team_rotation.rs::assert_final_rotation_invariants`

**完整调用链**：[docs/调用链/debug与验证.md](../调用链/debug与验证.md)

**模块 owner**：[docs/接口/错误与诊断.md](../接口/错误与诊断.md)；[docs/求解错误分析报告.md](../求解错误分析报告.md)；[docs/文档生命周期.md](../文档生命周期.md)

**机制引用**：[docs/知识库引用.md#不可混淆的四种内容](../知识库引用.md#不可混淆的四种内容)

**知识库原文（vault @ 977b6ab0cc0430053194f269d7b3ee0242e7be48）**：`docs/1-基础设定/README.md`

**验证**：`infra-cli debug rerun --bundle <debug-bundle.json>`；`python3 scripts/codex/check_repository_facts.py`
