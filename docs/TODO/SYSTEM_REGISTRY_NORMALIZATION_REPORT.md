# 组合体系规范化审计报告

> 状态：doing
> 来源：用户要求；`data/MECHANICS_REGISTRY.csv` 是全干员基建技能原文真源
> 目标：把“中间产物 / 跨站 meta / 同站组合 / 核心优先 / 散件工具人”重新分层，避免继续把不同语义塞进同一张组合表。
> 进度：Phase 0/1 已落地；Phase 2 起的跨站 / 全局资源拆清仍待做。

## 1. 审计结论

当前项目的原始设计是正确的：机制、体系、组合、散件应分层处理。现在的主要问题不是某个公式错，而是部分数据和文档把不同耦合半径混成了 `base_systems.json` 的 fixed registry：

- `witch_long_beta` 被写成 fixed same-station registry，但实际应是“巫恋核心 + 龙舌兰优先 + 裁缝 beta/alpha/普通工具人 fallback”。
- `blackkey_closure` 被写成 fixed same-station registry，但实际应是“可露希尔核心 + 黑键/高效工具人优先”，黑键自己的感知链是 global resource，不应被等同为可露固定搭配。
- `docus_syracusa` 的完整形态是跨站体系：中枢八幡海铃 + 贸易但书/伺夜/贝洛内；但“但书必须优先上”本身是贸易核心优先策略，不应只在完整叙拉古 fixed 组合存在时才生效。
- `企鹅 / 推王 / 龙巫 / 可露` 这类同站或 producer-gated 组合不应在普通排班中抢在更高优先级核心前认领房间。
- `感知、人间烟火、木天蓼、魔物料理、情报储备、乌萨斯特饮、虚拟电站` 是全局资源或中间产物，不是“组合”；它们的归属应是 `skill_table scope=global` + `cross_facility/` + `resolve_base`。

这次小饼 xlsx 的排班错误就是分层错位的典型症状：编排层 fixed registry 过早占房/占人，导致但书、可露希尔、龙巫这些核心没有按优先级进入排班。

## 2. 原始技能表事实

`data/MECHANICS_REGISTRY.csv` 当前有 727 条技能记录，字段为：

| 字段 | 含义 |
|------|------|
| `序号` | 原技能表行号 |
| `技能名` | PRTS/游戏技能名 |
| `工作设施` | 控制中枢、贸易站、制造站等 |
| `产物限定` | 赤金、战斗记录、职业/材料等限制 |
| `干员` | 一个或多个拥有该技能的干员 |
| `需求精英` | 精0/精1/精2 |
| `效率值` | 可直接摘出的效率值，非所有机制都有 |
| `游戏原文` | 机制真源文本 |

按设施计数：

| 设施 | 条数 | 本项目当前相关性 |
|------|------:|------------------|
| 制造站 | 109 | 高：制造效率、自动化、赤金/经验线 |
| 加工站 | 104 | 低：当前非主线 |
| 训练室 | 103 | 低：当前非主线 |
| 贸易站 | 91 | 高：订单效率、赤金订单、L3 shortcut |
| 控制中枢 | 88 | 高：global inject、跨站 producer |
| 宿舍 | 83 | 中：感知/魔物料理等 producer；心情排班非目标 |
| 会客室 | 67 | 低：线索主线当前非目标；会客等级可作为 selector |
| 办公室 | 43 | 中：絮雨感知 producer；公开招募效率非主线 |
| 发电站 | 39 | 高：无人机、虚拟电站、自动化依赖 |

结论：`MECHANICS_REGISTRY.csv` 是“技能原文真源”，但不是所有 727 条都应进入排班组合。当前项目的主线只应覆盖贸易、制造、发电、中枢、少量宿舍/办公室全局资源 producer；加工站、训练室、会客线索、心情连续排班仍是非目标。

## 3. 应有分层

### 3.1 L1/L2/L3 机制层

机制层回答“技能怎么算”，不回答“谁该先上岗”。

| 层 | 归属 | 例子 | 约束 |
|----|------|------|------|
| L1 atom/interpreter | `skill_table.json` + `trade/interpreter.rs` / `manufacture/interpreter.rs` | 巫恋低语、龙舌兰投资、可露特别订单、自动化、清流 | 只认 `buff_id`，不认干员名 |
| L2 mechanic | `order_mechanic.rs` / `gold_flow.rs` / `unit_output.rs` | 但书违约、赤金订单、单位产出 | 处理机制域最优解 |
| L3 shortcut | `trade_shortcuts.json` + `trade/shortcut.rs` | `gsl_witch_*`、`gsl_docus_*`、`gsl_closure_*` | 只做难 atom 化组合/锚点，不负责排班优先级 |

### 3.2 全局资源 / 中间产物

这类机制的共同点是：producer 与 consumer 不一定同房，资源进入 `LayoutContext.global` 或 `global_inject`。

| 资源 | 原文依据 | 应归属 | 当前状态 |
|------|----------|--------|----------|
| 感知信息 | 黑键 501、迷迭香 309、爱丽丝 134/136、车尔尼 135/137、絮雨 190/191 | `scope=global` + `cross_facility` | 已部分落地 |
| 无声共鸣 | 黑键 498/499/501、塑心宿舍等 | global resource consumer/read | 已部分落地 |
| 思维链环 | 迷迭香 301/302/309 | global resource consumer/read | 已部分落地 |
| 人间烟火 | 重岳 37、乌有 502 等 | `scope=global` + `cross_facility` | 已部分落地，仍需清硬编码 |
| 魔物料理 | 森西 133 | 宿舍 producer + global resource | 已部分落地 |
| 木天蓼 | 火龙S黑角 29、麒麟R夜刀 35 | 中枢 producer + global resource | 已落地，但不应放进贸易 meta 互斥组 |
| 虚拟电站 | 森蚺 65、承曦格雷伊 391 | global/layout resource | 已部分落地 |
| 情报储备 / 乌萨斯特饮 | 灰烬、战车等 | global resource，当前非主线 | 不应混进贸易/制造组合 |

规则：全局资源不是 `base_systems` 的同站组合。排班可以为了资源 producer 落位，但资源结算必须由 `resolve_base` / `cross_facility` 完成。

### 3.3 跨站 meta

跨站 meta 的特征是：至少一个 producer 房间影响另一个 consumer 房间。

| 体系 | 原文依据 | 目标归属 | 说明 |
|------|----------|----------|------|
| 但书叙拉古完整链 | 八幡海铃 76；贝洛内 441/505/506；伺夜 478；但书 433/434/436 | `cross_station_meta` | 八幡海铃、伺夜、贝洛内是跨站体系的一部分；但书核心优先不应依赖完整链存在 |
| 灵知孑喀兰 | 灵知 78；孑 444/445；银灰/讯使/崖心 466-468；琳琅诗怀雅 512 | control producer + L1 自然搜索 | 不应 active L3；`gsl_ling_jie_yaxin` 只做参考锚点 |
| 推王格拉斯哥 | 戴菲恩 79；摩根 507；维娜 482/483 | producer-gated station role | 可以有 segment，但优先级低于但书/可露/龙巫核心 |
| 怪猎木天蓼 | 火龙S黑角 29/75；麒麟R夜刀 35/72 | global resource/inject | 不是贸易 meta_chain |
| 自动化金线 | 承曦格雷伊 391；森蚺 65；温蒂/森蚺自动化 278-280；清流 281 | production-line system | 是制造/发电闭环，不是 same-station trade registry |

### 3.4 同站组合

同站组合只描述“同一个房间内必须或倾向一起上”的关系。

| 类型 | 例子 | 应用方式 |
|------|------|----------|
| fixed | 极少数三人固定且缺一不可的成套站 | `base_systems fill_mode=fixed` |
| bond | 德克萨斯+拉普兰德、能天使+蕾缪安、摩根+推进之王 | 固定二人核，第三人搜索/工具人 |
| core priority | 但书、可露希尔、巫恋/龙舌兰 | 核心必须优先进入候选，队友由搜索选 |
| tag scaler | 新约能天使拉特兰、维娜格拉斯哥 | L1/tag 搜索，不宜硬编码固定三人 |

### 3.5 散件工具人

散件只用于缩小搜索空间：

| 文件 | 角色 |
|------|------|
| `data/standalone_roster.json` | 贸易/制造/发电/中枢白名单 |
| `try_filter_standalone` | 过滤后不足则回退全池 |

规则：散件不进 `base_systems.json`，不参与 meta 互斥。

## 4. 现有数据表审计

### 4.1 `base_systems.json`

| id | 当前归类 | 建议归类 | 问题 / 动作 |
|----|----------|----------|-------------|
| `docus_syracusa` | `cross_station` fixed | `cross_station_meta` + `docus_core` role | 完整叙拉古链可保留，但但书核心优先应独立；缺伺夜/贝洛内时也要上但书配最高效率工具人 |
| `automation_group` | `cross_station` | `production_line_system` | 方向正确，但应标明这是制造/发电闭环，不与贸易 meta 同 schema |
| `ling_jie_karlan` | `cross_station` control-only | `control_producer + natural_search` | 方向正确；继续保持 L1 自然搜索，不 active L3 |
| `witch_long_beta` | `same_station` fixed | `core_priority` / `witch_role` | 当前 fixed 错位；应支持 beta/alpha/blank fallback |
| `blackkey_closure` | `same_station` fixed | `closure_core_priority`，黑键只是优先工具人/感知 consumer | 当前 fixed 错位；可露不等于黑键+吉星固定站 |
| `gongsun_greyy2_power_line` | `same_station` | `global/layout power resource` | 多发电站固定 slot 不是 same-station 组合；应单独归 power-line policy |
| `lungmen_manu_pair` | `cross_station` | `control_global_inject` | 与 `control_manu_injectors` 语义接近，建议合并入口 |
| `pinus_sylvestris` | `cross_station` | `production_line_system` | 可保留，但应标明是经验/赤金产线体系，不是 generic meta |
| `penguin_exusiai_lemuen` | `same_station` fixed-ish | `same_station_bond` | 二人 bond + 第三工具人；不应在高优先贸易核心前抢站 |
| `penguin_texangel_e2` | `same_station` fixed-ish | `same_station_bond` | 同上 |
| `penguin_texlap_e0` | `same_station` fixed-ish | `same_station_bond` | id 名称历史兼容；德克萨斯 E2 不应被 id 误导 |
| `vina_lungmen` | `cross_station` fixed | `producer_gated_bond/tag_role` | 戴菲恩 producer + 格拉斯哥同站加成；可保留 segment，但低优先 |
| `snhunt_monhun_control` | `cross_station` + `meta_chain` | `global_resource_producer` | 木天蓼/全局注入不是贸易 meta_chain；应从 `meta_chain` 互斥语义中移出 |

### 4.2 `trade_segments.json`

| segment / role | 当前用途 | 建议 |
|----------------|----------|------|
| `docus_syracusa` | producer-gated shortcut | 保留，但作为但书 role 的“完整链优先 step” |
| `blackkey_closure` | producer 空、consumer fixed | 降级为可露核心搜索的 preferred shortcut，不应代表 fixed system |
| `penguin_*` | active segment | 保留 L3 锚点，但不要让 registry 早于核心优先抢站 |
| `vina_lungmen` | 戴菲恩 producer-gated | 保留，但归低优先 role |
| `roles.docus` | segment -> docus solo -> unfiltered | 方向正确，但 `unfiltered` 不应导致无但书时误选 plain；应增加 `must_include=但书` fallback 或显式 core role |
| 缺失 `roles.closure` | 可露核心优先目前在 `assign.rs` 临时代码 | 应补角色链 |
| 缺失 `roles.witch` | 巫恋核心优先目前在 `assign.rs` 临时代码 | 应补角色链，支持 `gsl_witch_*` fallback |

### 4.3 `trade_shortcuts.json`

| shortcut | 当前语义 | 建议 |
|----------|----------|------|
| `gsl_witch_*` | 巫恋核分档 | 正确；不应只拿 `gsl_witch_long_beta` 进编 |
| `gsl_closure_tier*` | 可露希尔分档 | 正确；应由 closure role 选择 |
| `gsl_docus_syracusa` | 完整叙拉古链 | 正确；跨站 segment 命中 |
| `gsl_docus_solo` | 但书单走 | 正确；但排班 role 要强制包含但书 |
| `gsl_ling_jie_yaxin` | 参考锚点 | 正确；不 active 匹配 |
| `gsl_blackkey_closure` | 可露+黑键高配锚 | 可保留为 closure preferred shortcut，不能代表唯一固定组合 |
| `gsl_penguin_*` | 企鹅 bond 锚 | 可保留；排班层应低于核心 meta |
| `gsl_vina_lungmen` | 推王组锚 | 可保留；排班层应低于核心 meta |

### 4.4 `assign.rs`

当前存在一段修小饼问题的临时逻辑：

- 跳过 `blackkey_closure`、`witch_long_beta`、`penguin_*`、`vina_lungmen` 等 registry fixed 认领；
- 在贸易余站里按 `但书 -> 可露希尔 -> 巫恋 -> plain` 搜索；
- `但书` 仍走 `roles.docus`；
- `可露/巫恋` 的 role 逻辑临时写在 `assign.rs`。

这解决了眼前排班错误，但不是最终架构。最终应把这段逻辑迁移到 role policy / system registry，而不是长期堆在宏观排班函数。

## 5. 推荐目标数据模型

可以继续沿用 `base_systems.json`，但需要把语义字段补清楚。建议每条 registry 至少具备：

| 字段 | 建议值 | 说明 |
|------|--------|------|
| `kind` | `global_resource` / `cross_station_meta` / `production_line_system` / `same_station_bond` / `core_priority` / `standalone` | 比当前 `tier` 更准确 |
| `facility_scope` | `global` / `cross_station` / `same_station` / `single_facility` | 耦合半径 |
| `fill_mode` | `fixed` / `bond` / `core` / `producer_only` / `greedy` | 怎么落位 |
| `source_refs` | CSV 序号列表 | 每条体系必须能回指 `MECHANICS_REGISTRY.csv` |
| `role_id` | 如 `docus` / `closure` / `witch` / `vina` | 贸易核心角色 |
| `priority_policy` | 命名 policy | 不再用匿名调权 |
| `l3_shortcuts` | shortcut id 列表 | 锚点和匹配器 |

若不想破坏现有 schema，可先新增 `data/system_registry_audit.json` 或在文档中维护表格，等迁移稳定后再改 JSON schema。

## 6. 建议迁移阶段

### Phase 0：冻结真源和命名

状态：已落地。

- 明确 `MECHANICS_REGISTRY.csv` 是全干员技能原文真源。
- 每个组合/体系文档必须写 `source_refs`。
- 文档中停止使用“固定组合”描述 core priority。

验收：

- `docs/ORCHESTRATION_LAYER.md`、`docs/BASE_ASSIGNMENT.md`、`docs/SCHEDULE_ROTATION.md` 中 `witch_long_beta fixed`、`blackkey_closure fixed` 的描述被替换。

### Phase 1：贸易 meta 角色化

状态：已落地。`trade_segments.json` 已补 `roles.docus` / `roles.closure` / `roles.witch`，`assign_shift` 主路径跳过旧 fixed 抢站条目，由 `search/role_pick.rs` 执行核心优先。

目标排序：

1. `docus`：完整叙拉古链优先；否则但书单走，强制包含但书；否则失败，不退化成无但书 plain。
2. `closure`：强制包含可露希尔；优先黑键/高效工具人；按 `gsl_closure_tier*` 分档。
3. `witch`：强制包含巫恋；龙舌兰优先；裁缝 beta/alpha/blank fallback。
4. `same_station_bond`：企鹅、推王等低优先 bond。
5. plain：散件 C(n,3)。

动作：

- 在 `trade_segments.json` 或新的 role registry 中补 `roles.closure`、`roles.witch`。
- 删除 `base_systems.json` 中 `witch_long_beta`、`blackkey_closure` 的 fixed 认领，或降级为 role preference。
- 把 `assign.rs` 临时函数 `pick_closure_trade_hit` / `pick_witch_trade_hit` 迁走。

验收：

- 小饼 xlsx：轮换中稳定出现但书、可露希尔、巫恋+龙舌兰。
- 缺贝洛内/伺夜时，但书仍会进贸易站。
- 缺黑键/吉星时，可露仍会进贸易站。
- 缺裁缝 beta 时，龙巫走 alpha/blank fallback。

### Phase 2：跨站 / 全局资源拆清

动作：

- `snhunt_monhun_control` 从 `meta_chain` 语义中移出，归 global resource producer。
- `gongsun_greyy2_power_line` 标为 power/layout resource policy，不叫 same-station。
- `automation_group`、`pinus_sylvestris` 标为 production-line system，避免与贸易 meta 共享解释。
- 继续清理 `resolve.rs` 中剩余按名硬编码，迁到 `scope=global` atom。

验收：

- 木天蓼、感知、人间烟火、魔物料理只通过 `resolve_base` / `cross_facility` 进入效率，不通过贸易 fixed 组合进入。

### Phase 3：制造同站 bond

候选来自 `MECHANICS_REGISTRY.csv`：

| 组合 | CSV 行 | 建议 |
|------|--------|------|
| 怒潮凛冬 + 乌萨斯学生自治团 | 224 | 战斗记录 bond/tag，可低优先 |
| 阿兰娜 + 温米 | 310 | 赤金 bond，需评估是否会抢自动化金线 |
| Miss.Christine + 酒神 | 311 | 战斗记录 bond，低优先 |

动作：

- 不急于 registry fixed；先建 L1/L3 回归，再决定是否进入排班。

### Phase 4：回归矩阵

必须覆盖：

- 全精2 243：完整叙拉古链仍能命中 `gsl_docus_syracusa`。
- 小饼类缺件账号：但书/可露/龙巫核心都能上。
- 无但书账号：不因 `roles.docus` unfiltered 误称为 docus。
- 缺 beta 裁缝账号：巫恋 fallback 正确。
- 可露无黑键账号：可露 fallback 正确。
- 灵知孑：仍是 L1 自然计算，`trade_shortcut=None`。
- 木天蓼/感知：通过 global resource，而非组合。

## 7. 当前最高风险点

| 风险 | 影响 | 优先级 |
|------|------|--------|
| 文档仍写 `witch_long_beta fixed` / `blackkey_closure fixed` | 后续 agent 会继续误改 | P0 |
| `base_systems.json` 混入 core priority、global resource、production line | 排班抢站抢人 | P0 |
| `roles.docus` 最后 `unfiltered` 可能语义不清 | 无但书时可能退化成 plain | P1 |
| 贸易 role policy 写在 `assign.rs` | 逻辑不可复用，后续难维护 | P1 |
| 木天蓼在 `meta_chain` 互斥组 | 把 global resource 当 trade meta | P1 |
| 控制中枢全制造注入有 `control_manu_injectors` 和 `systems` 两套入口 | 维护重复 | P2 |

## 8. 推荐下一步

先做 Phase 0 + Phase 1，范围只动贸易 meta 和文档：

1. 更新 `ORCHESTRATION_LAYER.md` / `BASE_ASSIGNMENT.md` / `SCHEDULE_ROTATION.md` 的错误 fixed 描述。
2. 把 `docus / closure / witch` 三个贸易 role 正式写入 role registry。
3. 从 `base_systems.json` 移除或降级 `witch_long_beta`、`blackkey_closure` 的 fixed 认领。
4. 把 `assign.rs` 的临时核心优先逻辑改成调用 role policy。
5. 补小饼 xlsx 对应的最小 operbox 回归。

这样可以先把这次严重错误的根因消掉，再处理自动化、红松、木天蓼等更大范围的体系归类。
