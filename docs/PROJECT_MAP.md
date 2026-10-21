# 项目地图（Agent / 开发者入门）

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/文档生命周期.md；docs/ORCHESTRATION_LAYER.md；docs/BASE_ASSIGNMENT.md；docs/INFRA_CLI.md；docs/排班模式.md
> 摘要：提供当前代码、数据和命令 owner 地图

> 本文是当前代码、数据和命令地图，不是每个 Agent 任务的无条件首读。先由 [AGENTS.md](../AGENTS.md) 选择 Skill；只有 owner、入口或调用链不明时才定向读取本文。领域语义见对应 canonical Markdown，文档位置未知时查 [INDEX.md](INDEX.md)。

## 使用方式

- 查业务规则：从 [INDEX.md](INDEX.md) 进入唯一 canonical，不从本文推导公式或 admission。
- 查代码 owner、命令入口或调用链：按本文对应表格定位，不要求通读。
- 查实现状态：以当前代码和生成 help 为准；本文只维护稳定 owner，不复制完整 wire schema、fixture 真值表或 top hit。
- 查历史方案：进入 `docs/ARCHIVE/`；开放工作只看已由当前任务恢复的 `docs/TODO/`。
- 按症状定位完整影响链：先查 [生成 Agent 路由](生成索引/Agent路由.md)，再进入 `docs/调用链/` 和对应 `docs/模块/`；本文只保留稳定 owner，不复制调用链正文。

## Workspace Owner

| 路径 | 单一职责 | 深读入口 |
|---|---|---|
| `crates/infra-core/` | 机制解释、候选、搜索、编排、排班与导出数据结构 | 本文 core/layout 表和各领域 canonical |
| `crates/infra-cli/` | argv/wire adapter、文件加载、调用编排和忠实输出；不写机制或重新选型 | [INFRA_CLI.md](INFRA_CLI.md) |
| `data/` | 运行时表、规则载体、fixture 和参考快照；不裁决业务语义 | 本文 data 表 |
| `scripts/` | 数据构建、导入、审计、发布和证据工具 | 本文工具表；证据协议见 [scripts/codex/README.md](../scripts/codex/README.md) |
| `release/` | 发布包配置、生成器和发布夹具 | [release/README.md](../release/README.md) |
| `docs/` | canonical、current reference、ADR、active change 与 archive | [文档生命周期](文档生命周期.md) |

## 关键生产链

```text
layout / operbox / runtime data
  -> schedule_timed_rotation_request (timed path)
       -> assign_shift* (reference and shift construction)
  -> assign_shift* (single-shift path)
       -> build_plan
       -> run_shift_pipeline
            -> execute_plan
            -> build facility pools
            -> control / producer / power / trade / manufacture fill and search
            -> resolve snapshots and final BaseAssignment
  -> TeamRotationReport / profile / MAA / CLI or Worker output
```

- Layout 的概念主路径是 `build_plan -> execute_plan -> fill -> resolve`；`layout/assign/pipeline.rs` 是阶段顺序与 resolve 时机的实现 owner。
- L1 interpreter 只认 `buff_id`；L2 处理领域机制；L3 shortcut 只结算实际组合，不负责体系选型或进编。
- `schedule_timed_rotation_request` 接受六个具名 profile，并支持普通二/三班 topology 按序指定时长；菲亚 `8/8/4/4`、深海 `7/5/7/5` 仍为具名四班路径。独立龙巫二班已从产品支持中删除，旧 CLI/Worker 请求明确失败；`schedule_team_rotation` 是默认 ABC 薄包装。模式边界见 [排班与轮换](模块/排班与轮换.md)，当前实现见 [定时轮换](调用链/定时轮换.md)。
- CLI 和 export 只消费 core 结果；`plan`、`plan.compute` 与 legacy Worker `plan` 共享 `commands/plan_compute.rs` 的一次 rotation 结果。

## `infra-core` 模块索引

本表以 `crates/infra-core/src/lib.rs` 的 `pub mod` 集合为边界。职责是 owner 定位，不替代模块 rustdoc 或领域 canonical。

| 模块 | 职责 |
|---|---|
| `bake` | Bake catalog 生成、加载、兼容门禁和 runtime warmup |
| `box_profile` | 账号练度画像、差距和建议动作投影 |
| `candidate` | 跨域通用候选来源、设施类型和 metric 载体 |
| `cross_facility` | 收集并执行 `scope=Global` atom，形成跨设施资源快照 |
| `eff_ramp` | 时间爬升效率计算 |
| `efficiency` | 生产域唯一 `Efficiency` 千分整数类型 |
| `error` | crate 统一 `Error` / `Result` |
| `global_resource` | 具名全局资源、转化和中枢注入 manifest |
| `instances` | `operator_instances.json` 加载、tier/stepwise buff 归属解析 |
| `mood` | 心情净消耗、恢复和工作时长 ETA 内核 |
| `operbox` | 玩家练度盒 JSON/xlsx 输入与默认 fixture 路径 |
| `pool` | 各设施候选池和 standalone/full-pool 边界 |
| `profile` | 供性能画像使用的轻量热路径计数器 |
| `response_dependency` | 动态 producer 规则、响应依赖与 reverse closure |
| `recycle` | 回收站设施容量、技能效率与房间结算参数；选人由 schedule 普通房间 admission 负责 |
| `roster` | 维护用设施 roster CSV 输入 |
| `schedule` | timed profiles、ABC、shift bind、班次结算和报告 |
| `scoring` | 具名非生产 policy 与分量，不冒充生产效率 |
| `search` | 贸易、制造、中枢、发电和 role/joint 候选搜索 |
| `skill_table` | `skill_table.json` 加载、数据路径与嵌入 fallback |
| `support_facility` | 办公室/会客室静态求值的共享类型与 registry |
| `tier` | `PromotionTier` 技能解锁档 |
| `training_advice` | 练卡：玩家 knowledge/player 层生成基础名单、组合完成度与 ≤10 建议；v2 机器诊断继续兼容 |
| `types` | EffectAtom 的 phase、selector、condition、action 等共享类型 |
| `notes` | 结构化 Note 类型和可选诊断附注；不参与机制求解 |
| `control` | 中枢求值、效果账本与全局注入写回 |
| `export` | MAA 排班 JSON 投影；不重新计算机制 |
| `layout` | 蓝图、编制、Plan、填房、resolve 和 workforce |
| `manufacture` | 制造 L1、单房求解和配方结果 |
| `meeting` | 会客室显式编制静态求值 |
| `office` | 办公室显式编制静态求值 |
| `power` | 发电站求值和布局写回 |
| `trade` | 贸易 L1/L2/L3、单房求解和单位产出 |

### `layout/` Owner

| 路径 | 职责 |
|---|---|
| `layout/assign.rs` | `assign_base_greedy` / `assign_shift*` facade；构造 skip set、调用 `build_plan` 并进入 pipeline |
| `layout/assign/pipeline.rs` | 单班阶段顺序、`execute_plan`、建池、动态 producer 比较、逐设施 fill 和 resolve 时机 |
| `layout/assign/run.rs` | `AssignmentRun` 可变状态、resolve snapshot 与阶段计时 |
| `layout/assign/commit.rs` | assignment 提交和 `used` 同步 |
| `layout/assign/{control_fill,producer_fill,power_fill,trade_fill,manufacture_fill,team_fill}.rs` | 各设施或队伍的实际填房 owner |
| `layout/orchestrate/rules.rs` | 声明式 Rule gate/role/relation 编译器 |
| `layout/orchestrate/select.rs` | 高优先 Rule、legacy registry、late competitive Rule 汇合并生成 Plan |
| `layout/orchestrate/plan.rs` | 已解析 `AssignmentPlan`、anchor、bind、dependency 和 reserve |
| `layout/orchestrate/execute.rs` | 执行已解析 placement；不按 system/rule id 重判 |
| `layout/resolve.rs` | 蓝图 + assignment -> workforce、全局资源、注入和 resolved rooms |
| `layout/system.rs` | `base_systems.json` legacy registry 解析与兼容 helper |

### `trade/` Owner

| 路径 | 层级 / 职责 |
|---|---|
| `trade/input.rs` | `TradeOperator` / `TradeRoomInput` 输入类型 |
| `trade/interpreter.rs` | L1：按 Phase 解释通用 EffectAtom |
| `trade/gold_flow.rs` | L2：虚拟赤金线链 |
| `trade/order_mechanic.rs` | L2：订单分布和机制等效效率 |
| `trade/shortcut.rs` | L3：按实际组合命中表化规则 |
| `trade/segment.rs` | producer/consumer 链段与 core role fallback 数据 |
| `trade/efficiency.rs` | 贸易直接效率分量组装 |
| `trade/solver.rs` | 串联 L1/L2/L3，输出 `TradeResult` |
| `trade/unit_output.rs` | 社区单位产出、无人机和日产量换算 |

## `infra-cli` 命令

该首表由 `scripts/codex/check_repository_facts.py` 与顶层 dispatch、`layout` 子命令集合对账。详细参数和输出契约见 [INFRA_CLI.md](INFRA_CLI.md) 与前端文档。

| 命令 | 用途 |
|---|---|
| `plan` | 用户完整模拟入口：账号画像 + timed rotation + 可选 MAA/JSON；不含练卡建议 |
| `advice --operbox <path>` | legacy v2 机器诊断练卡、确定性答卷或 explain/RAG 输入；玩家练卡建议必须从 `serve plan.compute.training_advice` 消费 |
| `verify --case <id>` / `verify --all` | 执行贸易回归和单位产出锚点 |
| `pool [--trade] [--manufacture]` | 打印设施候选池统计与跳过原因 |
| `search trade [--top N]` | 贸易候选搜索 |
| `trade yield <fixture>` | 单站单位产出探测 |
| `bench --operbox <path>` | 固定布局的贸易/制造搜索基准 |
| `bake [all\|trade\|manufacture]` / `bake validate` / `bake verify` | 生成、兼容校验和响应验证 Bake catalog |
| `debug rerun --bundle <path>` | 重放反馈 debug bundle，并可按房间、干员或班次缩小输出 |
| `serve` | 启动前端 JSON line 常驻 worker；机器主入口为内联 `plan.compute` |
| `layout test` | 自定义蓝图的单班搜索或指定 assignment 探测 |
| `layout team-rotation` | timed rotation 与 MAA 导出，不附带账号画像 |
| `layout analyze` | 练度 box profile 分析 |
| `layout eval` | 静态评估指定编制 |
| `profile layout-full` / `profile analyze-compare` / `profile bake-dependencies` | 性能画像和 Bake 依赖报告 |

### `infra-cli` 源码 Owner

| 路径 | 职责 |
|---|---|
| `src/main.rs` | 进程入口和顶层子命令路由；部分 legacy pool/search/trade/bench 编排仍在此 |
| `src/commands/plan.rs` | `plan` argv/文件适配、Plan 结果写出和人类输出 |
| `src/commands/plan_compute.rs` | `plan` / `serve` 共用的单次 rotation + profile/MAA 编排 |
| `src/commands/serve.rs` | `plan.compute` 内联协议和 legacy 路径适配 |
| `src/commands/layout.rs` | `layout test` / `team-rotation` / `analyze` / `eval` adapter |
| `src/commands/advice.rs` | v2 练卡规则和 operbox adapter |
| `src/commands/bake.rs` | Bake 生成、validate 和 verify adapter |
| `src/commands/debug.rs` | debug bundle 校验、重放和诊断输出 adapter |
| `src/commands/profile.rs` | layout、analysis 和 Bake dependency profile |
| `src/commands/verify.rs` | `expect_rule_id` 回归断言和 fixture 选择 |
| `src/verify/{cases,fixtures}.rs` | CSV case 加载与硬编码最小房间 fixture |
| `src/output.rs` | CSV/text/JSON 格式化；不求解或重排候选 |

仓库标准 243 fixture 位于 `data/fixtures/243/{layout.json,operbox_full_e2.json,schedule_export.json}`。用户说“跑一遍模拟”时默认使用 `plan`；只需排班时使用 `layout team-rotation`，具体命令见 [Debug 指南](MAINTENANCE_MODE.md#22-复现入口)。

前端与发布路径：[FRONTEND_CLI.md](FRONTEND_CLI.md)、[FRONTEND_SERVE_GUIDE.md](FRONTEND_SERVE_GUIDE.md)、[release/README.md](../release/README.md)。Next BFF 使用 `serve` / `plan.compute`；一次性调用使用 `plan` / `--maa-out`。

## `data/` Owner

下列文件只承载运行时实现、fixture 或核对材料。业务变更必须先统一对应 canonical。

| 路径 | 运行时角色 |
|---|---|
| `skill_table.json` | `buff_id -> SkillDef / EffectAtom[]`；空 atoms 表示委托领域引擎 |
| `operator_instances.json` | 干员、练度档、设施和 buff 的归属映射 |
| `orchestration_rules.json` | 当前声明式 Rule/alternative/role/relation catalog |
| `base_systems.json` | 未迁移体系的 legacy compatibility registry |
| `producer_rules.json` | 动态 producer 响应依赖声明 |
| `trade_segments.json` / `trade_shortcuts.json` | L2/L3 链段、core role 和实际组合结算表 |
| `standalone_roster.json` | 具名 standalone 搜索边界；不替代普通制造 full pool |
| `mood_model.json` | 心情消耗与恢复参数 |
| `support_skill_registry.json` | 办公室/会客室静态技能 registry |
| `training_recommendations.json` | **旧** v2 机器诊断规则载体，不是玩家真源、不是 solver 候选池；玩家层使用独立的 `training_advice_knowledge.json` |
| `REGRESSION_CASES.csv` / `UNIT_OUTPUT_ANCHORS.csv` | `verify` 回归和单位产出锚点 |
| `fixtures/` / `layout/` / `schedule_243/` | 真实入口 fixture、蓝图模板和排班样例 |
| `tags/` | Selector/规则使用的阵营与结构 tag |
| `baked/` | 本地 Bake catalog；schema/输入不兼容时必须拒绝并走 live 路径 |
| `feedback_regression_seeds/` | 从反馈提炼的回归种子，不拥有领域语义 |
| `prts_*` / `MECHANICS_REGISTRY.csv` | 外部原文快照和历史归档核对材料 |

## `scripts/` Owner

| 类别 | 入口 |
|---|---|
| 技能/规则数据构建 | `build_*_skill_table.py`、`generate_support_skill_registry.py`、`merge_*.py` |
| operbox/roster 导入 | `xlsx_to_operbox.py`、`inspect_xlsx_operators.py`、`build_roster_from_operbox.py` |
| 数据审计 | `audit_*.py`、`check_trade_roster.py`、`verify_regression.py` |
| fixture/参考材料 | `build_243_schedule_fixture.py`、`fetch_schedule_layout.py`、`save_prts_*` |
| 练卡推荐 | `render_training_recommendations.py`、`migrate_training_recommendations_v2.py` |
| Bake/性能 | `bake_and_verify.sh`、`analyze_manu_recipes.py`、`stats_facility_operators.py` |
| Evidence/lifecycle | `scripts/codex/`；统一入口为 `run_evidence.sh` |
| 发布与反馈 | `build_release_linux.sh`、`sync_feedback.py` |

本文不维护 scripts 全量文件表；新增工具应放入最接近的类别，并由脚本自身 help、README 或调用方证明具体参数。

## 验证入口

| 层级 | 主要入口 |
|---|---|
| Rust 单元/集成 | 各模块 `mod tests`；按 owner 运行 `cargo test -p infra-core <filter>` |
| CLI 回归 | `infra-cli verify` + `data/REGRESSION_CASES.csv` + `data/UNIT_OUTPUT_ANCHORS.csv` |
| 真实 layout/plan | `data/fixtures/243/` + `plan` / `layout test` / `layout team-rotation` |
| 文档 lifecycle/link/命令地图 | `docs_inventory.py --check`、`check_repository_facts.py` |
| 可复现交付证据 | `scripts/codex/run_evidence.sh` -> `target/codex-runs/<task>/` |

## 常见 Owner 查询

本表给出从任务语义到首个实现 owner 的有界路径，不枚举全部调用方。到达具名文件后再做符号搜索；如果仍需跨目录通读，说明这里缺少 owner 路由。

| 已选定的任务范围 | 先看 | 实现 owner |
|---|---|---|
| EffectAtom/L1 机制 | [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) | `types.rs`、对应 interpreter、skill/instance 数据 |
| 贸易 L2/L3 | [INTERNAL/TRADE_INTERPRETER.md](INTERNAL/TRADE_INTERPRETER.md)、[INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md) | `trade/{gold_flow,order_mechanic,shortcut,segment}.rs` |
| 制造 full_pool / standalone 与填房 | [MANUFACTURE_STATUS.md](MANUFACTURE_STATUS.md)、[PERFORMANCE_ENGINEERING.md](PERFORMANCE_ENGINEERING.md) | `layout/assign/manufacture_fill.rs::manu_options` 设置排班 `full_pool=true`；`search/manufacture.rs::search_manufacture_single_recipe` 只在 `false` 应用 standalone；体系 required 路径见 `layout/orchestrate/rules.rs`，Bake gate 见 `bake.rs` |
| 中枢求值 / 普通补位 | [CONTROL_CENTER_ASSIGNMENT.md](CONTROL_CENTER_ASSIGNMENT.md) | `control/`、`search/control.rs` |
| 动态 producer admission / 依赖 / 同班 | [CONTROL_CENTER_ASSIGNMENT.md](CONTROL_CENTER_ASSIGNMENT.md)、[SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md) | `data/producer_rules.json` 声明 admission/target/relation → `response_dependency.rs` 加载并规范化依赖 → `layout/assign/pipeline.rs` / `layout/orchestrate/plan.rs` 产出事实 → `schedule/shift_bind.rs` 消费 |
| 办公室/会客室静态求值与出口 | [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md)、[FRONTEND_CLI.md](FRONTEND_CLI.md) | 当前 `support_facility.rs` → `layout/resolve.rs` → `commands/layout.rs::layout_eval_cmd`；未实现的 `plan.compute` 扩展点是 `schedule/rotation_types.rs::TeamShiftResult` → `commands/serve.rs::RotationShiftSummary` |
| 单班 Plan/填房 | [ORCHESTRATION_LAYER.md](ORCHESTRATION_LAYER.md)、[BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md) | `layout/orchestrate/`、`layout/assign/` |
| timed rotation/MAA | [排班模式](排班模式.md)、[SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md)、[排班职责地图](模块/排班职责地图.md) | `schedule/team_rotation.rs` 调度；`rotation_types.rs` 公共报告；`room_admission.rs` 候选试算/ETA helper；`rotation_scoring.rs` 菲亚后的普通班次重算；`shift_bind.rs`、`cycle_mood.rs`、`dorm_finalizer.rs` 分别负责绑定、周期心情、宿舍；`export/maa.rs` 只投影 |
| argv/Worker Plan 汇合 | [INFRA_CLI.md](INFRA_CLI.md) | `commands/{plan,plan_compute,serve}.rs` |
| 练卡推荐 | [练卡推荐相关](练卡推荐相关/README.md) | `training_advice/`、`commands/advice.rs`；旧 `training_recommendations.json` 仅 diagnose |
| Bake/性能 | [PERFORMANCE_ENGINEERING.md](PERFORMANCE_ENGINEERING.md) | `bake.rs`、`pool/standalone.rs`、`commands/{bake,profile}.rs` |
| 文档角色/关闭任务 | [文档生命周期](文档生命周期.md) | `docs_inventory.py` 与 generated INDEX/TODO regions |

## 渐进式读取边界

1. 根 [AGENTS.md](../AGENTS.md) 负责任务分类和硬门禁。
2. [INDEX.md](INDEX.md) 负责领域文档和 canonical 导航。
3. 本文只在 owner、入口或调用链未知时定向读取，不再复制上述两份文档的路由表。

大文件优先用 `docs/INTERNAL/` 地图或符号搜索定位；不必通读 interpreter、output、PRTS HTML、xlsx、Bake 二进制或历史 registry。
