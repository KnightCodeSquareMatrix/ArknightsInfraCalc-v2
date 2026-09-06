# 编排层重构路线图（Orchestration Layer）

> 文档角色：canonical
> 生命周期状态：current
> 领域键：architecture.orchestration
> 当前真源：self
> 摘要：裁决 System 到 Plan 到 Execute 的编排契约

> **状态**：**2026-07-15 通用规则编译器主路径已落地**；旧专用 evaluator 已删除，不再扩展“一体系一个函数”
> **目标**：把业务体系写成角色、备选方案、关系和资源门槛，由同一个规则编译器产出完全解析的 **Rule → Candidate → Plan → Execute**；普通制造软同房仍由全合法候选池 + L1 solver 按实际效率发现。
> **背景讨论**：同房组合、跨房体系、global effect 三套入口搅在 `assign_shift` 里是乱源，不是机制太多。
> **历史旧稿**：[orchestration_engine_design.md](ARCHIVE/plans/orchestration_engine_design.md)。当前规则只枚举每条声明的有限 alternatives，并用显式 objective 或 policy 顺序裁决；仍不采用全编制笛卡尔积或匿名跨域总分。动态 producer 后续使用评分 canonical 声明的具名、可分解跨域 policy。

---

## 1. 问题陈述

历史上 `layout/assign.rs` 的 `assign_shift` 同时承担了多层职责；当前主路径已收敛为 `build_plan -> execute_plan -> fill_greedy`，下表记录边界：

| 职责 | 应在哪 | 当前边界 |
|------|--------|------|
| 选跨站体系 / 固定 bond / 条件硬锚点进编 | **通用规则编译器** | 规则目录实例化有限 alternatives，容量预检和资源 gate 后生成 `AssignmentPlan`；不按体系名分派函数 |
| 贸易核心优先 / 填散件第三人 | **role policy + 搜索（子集）** | `trade_segments.roles` + `search/role_pick.rs`；`assign_trade_remainder` / 制造 / 发电贪心填空房 |
| 算全局池 / 中枢注入 | **resolve** | `resolve_base` + `cross_facility` / `global_resource` |
| 算同房效率 / 产量 | **L1–L3 solve** | 不负责保证硬体系进编；普通制造在全合法候选池中用同房结算与 `final_efficiency` 排序，自然发现软组合 |

该重构要避免的后果：组合表化后又被 search 打散（如黑键可露链 vs 但书链）、`flat_eff_hint` / 并站 solve 分等补丁继续扩散。

---

## 2. 三种耦合范围 → 三种职责（不混层）

游戏机制（见 `data/MECHANICS_REGISTRY.csv`）可按**耦合半径**分类：

| 范围 | CSV 特征 | 编排 / 运行时 |
|------|----------|----------------|
| **硬同房 bond** | 用户确认必须同房 / 固定核心 | 声明式 role / `room_group` 或兼容 registry bond + L3 `shortcut` 回归锚点 |
| **普通制造软同房** | 标准化、仓容、莱茵技能等效率耦合 | 不注册 System；全合法普通制造池做 `C(n,k)`，L1 solver 按 `final_efficiency` 发现 |
| **跨房体系** | 中枢/宿舍 producer → 贸易/制造 consumer | **同一条 Rule alternative**，多设施 role + relation / resource gate |
| **全基建池** | 感知、人间烟火、木天蓼、虚拟电站 | **不是组合**；`resolve_base` → `GlobalResourcePool` + `cross_facility` |

**与 `OperatorTier` 枚举的对应关系**（`crates/infra-core/src/layout/tier.rs`）：

| 耦合范围 | OperatorTier | 分配阶段 |
|----------|-------------|----------|
| 跨房体系 | `CrossStation` | 第 1 轮 `select_registry_systems` |
| 同房 bond | `SameStation` | 第 2 轮 `select_registry_systems` |
| 普通制造候选 | `Standalone` | 不在 registry；排班使用全部合法普通制造池做 `C(n,k)`，由 solver 自然排序 |

`base_systems.json` 中每个 System 通过 `"tier"` 字段声明归属，`select_registry_systems` 据此分两阶段贪心：
先跨站、后同站，`exclusive_group` 互斥态跨轮共享。

**原则**

- **规则不重复机制公式**：候选需要比较时调用现有 room solver / `resolve_base` 读取正式结果，不在编排层手写效率公式；不同域只能使用评分 canonical 声明的具名、可分解权重，不能由编排层新增匿名综合分。
- **硬体系与普通制造分界**：L3 / System 可承载已确认硬组合；普通制造软同房不表化，search 对全合法普通候选自然排序。
- **global effect 不参与进编**：编制定完后再 `resolve` 写池。

---

## 3. 目标流水线

```
assign_shift()
  └─ orchestrate::compile_rules(operbox, blueprint, preferences) → PlanCandidate[]
       └─ resolve/check gates/select alternatives → AssignmentPlan（实际干员与房间已确定）
            └─ orchestrate::execute_plan(plan) → BaseAssignment
                 └─ fill_greedy(search_peers / remaining empty rooms)

resolve_base(assignment)   // 与 assign 分离，CLI 评分 / verify 才走
  ├─ control → GlobalInjectManifest
  ├─ cross_facility → GlobalResourcePool
  └─ per-room solve（L1 → L3 → L2 → production）
```

模块位置（当前）：

```
crates/infra-core/src/layout/orchestrate/
  mod.rs
  rules.rs     # 唯一通用 gate / role / relation 编译器
  plan.rs      # 已解析 AssignmentPlan、anchor、bind、dependency、continuous role
  select.rs    # 高优先 Rule → legacy registry → late competitive Rule 汇合
  execute.rs   # 只执行已解析 placement，不按 rule/system id 重判
```

`assign_shift` 当前主线为：seed → `build_plan` → `execute_plan` → producer/resolve → 发电 → 贸易 core role / plain → 制造贪心填充。贸易早于制造填充，避免但书、可露希尔、巫恋等核心和其工具人被制造站提前占用。

---

## 4. 通用规则 schema

体系不是执行函数。`data/orchestration_rules.json` 当前只声明：

- `alternatives`：按顺序尝试的有限备选方案，`explicit_only` 方案只接受 `--prefer`；
- `roles`：候选、最低练度、设施/配方、人数、`all_available` / `ordered`、同房 group、one-per-room、competitive、工作心情和休息去向；
- `relations`：禁同房、禁同站；`bind_roles` / `bind_all` 生成 exact shift bind，`active_dependencies` 生成单向在岗依赖；
- `exclude_operators`：alternative 激活后的路径级实际互斥；写入 plan 后由 fill / rotation 统一排除，不能只跳过 registry 又让普通补位重新引入；
- `gates`：设施数、订单数、配方数、可用/不可用干员，以及由临时 assignment 正式 resolve 的 global resource 门槛；
- `skip_registry_ids` / `skip_registry_ids_when_inactive`：与 legacy registry 的激活/关闭互斥，不在 `select.rs` 写体系 id 分支。

Mujica 是例外明确的低优先级中枢 fallback：正常 Rule、registry 与 late Rule 都未选中跨站体系时，才尝试完整五人（若叶睦、三角初华、丰川祥子、祐天寺若麦、八幡海铃）控制中枢 role。五人必须全部拥有，祥子必须精二，其余按实际练度结算；成功后五个 control anchor 和一条 `2/1` bind 同时进入 Plan。Mujica 不是普通候选比较器的体系评分，也不与已有跨站体系竞争；其余四人未成组时由 `control_exclusions` 阻止回流普通中枢，八幡海铃仍保留独立动态贸易 producer 路径。

Rosemary active 只关闭 legacy 纯烟火。感知附带分支是 priority 18 的独立声明式 rule：实际感知 ≥50 后仍必须让重岳、令、乌有三人完整落位并生成 bind；容量不足时自然关闭。不能在 select/rotation 另写 Rosemary id 分支。

`AssignmentPlan.rotation_reserves` 保存正式 role/solver 解析、但不进入 peak 的实际成员集合。H1/H2 的共同打包器先保留全部 exact bind 连通闭包，再优先按贸易站数量分配含贸易站闭包，纯制造/发电闭包才按总房间负载补平；它不拆闭包，也不按房号特判。`ResolvedRoleReserve.reuse_policy=once` 只在 H1/H2 中首个可行目标房使用一次；`reuse_policy=every_eligible_half` 要求两个 half 都存在目标，并在菲亚后处理前把同一组成员分别写入两个 gamma half。声明了 `require_pre_split_halves=true` 的 conditional pack 还必须在全部 exact bind 落位后的自然 H1/H2 打包结果中两边已有目标；否则计划编译器事务性重编译并降级为基础 alternative，不提交 pack 的 anchors、relations 或 reserve。已经提交的 reserve 若在基础 fill 中被覆盖仍是 schedule 不变量错误。reserve 不保护 `apply_fiammetta_return` 后的最终房间成员；最终 assignment / MAA 可以包含一次合法菲亚换人。

候选阶段解决所有 optional 选择；最终 `AssignmentPlan` 只包含已确定且可落位的 placement。`execute` 不得静默跳过 required placement，fill / rotation / export 不得再次判断体系。

`base_systems.json` 仍作为未迁移体系的兼容 registry，在高优先 Rule 与 late competitive Rule 之间执行；`layout/system_integrity/` 专用路径已删除。

### 4.1 贸易核心硬锚点

- 实际贸易站总数为 1，且该站是龙门币订单：可露希尔为首个 required trade anchor。
- 实际贸易站总数至少为 2，且存在龙门币订单：但书为首个 required trade anchor，并优先解析到最低等级的可用龙门币站。
- 多贸的后续金单 role 顺序为可露希尔、完整龙巫；可露希尔优先最高等级的可用龙门币站，完整龙巫必须由巫恋、龙舌兰和合法裁缝组成并进入三级站。
- 全部贸易站均为源石订单：不生成上述 anchor。
- 双贸易站的完整 Rosemary 三 cohort 条件成立时，声明式 packing 还必须把可露希尔与黑键解析到同一 B 房、与但书 A 房分离；C=巫恋+龙舌兰+合法裁缝由 γ 的正式 witch role 复用。A/B 还必须在全部 exact bind 的自然 H1/H2 打包中可分；不可分或需要 schedule 事后搬分量时整包降级。这里的同组关系只属于该可行 alternative，不是黑键全局固定可露希尔。
- 只固定核心本人，不固定房号和队友；叙拉古成员继续按实际跨站收益搜索。
- 该规则只读取实际设施数量和订单配方，不读取布局名称或房间编号。

## 4A. 旧 System schema（迁移兼容）

**同站组合与跨站体系都是 System**，差别只在 `slots` 数量与 `prerequisites`。

### 4.1 `tier` — 三层分配优先级

`base_systems.json` 中每个 System 必须声明 `"tier"` 字段：

| 值 | 含义 | 分配阶段 |
|----|------|----------|
| `cross_station` | 跨站体系（slot 跨多设施） | `select_registry_systems` 第 1 轮 |
| `same_station` | 同站组合（slot 在同一设施内） | `select_registry_systems` 第 2 轮 |

两轮共享 `exclusive_group` 互斥态。未注册的普通制造干员不走 registry；排班不套 standalone 名录，而从全部合法普通制造池做 `C(n,k)`。

对应 Rust 枚举：`crates/infra-core/src/layout/tier.rs` 的 `OperatorTier`，同时标注到 `RegistrySystemClaim`、`ActivatedSystem` 和各设施 `PoolEntry` 的 `tier` 字段。

### 4.2 `fill_mode`

| 值 | 含义 | Executor |
|----|------|----------|
| `fixed` | 整包落位 | 直接 `set_room`；迁移后只用于真正固定 bond，不承担一贸可露希尔/多贸但书 required anchor |
| `bond` | 二人锁死 + 第三人 | 固定 A+B，`pick_one` 填第三（例：德 E0+拉普兰德 **或** 能天使+蕾缪安；同干员不同 tier 须分叉 System） |
| `core` | 单人锚 + segment 池补满 | 仅用于未来非感知散件锚点；**黑键不走此路径** |
| `pick_one` | 列表选一 | 第一个可用干员 |
| `greedy` | Plain | 候选池内 `C(n,k)` 或发电 O(n)（制造排班直接使用全部合法普通制造池） |

`pick_one` 候选默认继承 slot 级 `"elite"` 要求；需要按候选区分精英化门槛时可写对象，例如 `{ "name": "海沫", "elite": 2 }`。对象候选还支持 `"max_elite"`，用于保留少量 E0-only / E1-only 历史锚点。

制造站 slot 可写 `"recipe": "gold" | "battle_record" | "originium"` 约束房间产物；用于自动化组这类“必须进赤金线”的体系。前端生成 layout 时房间编号可能与模板不同，优先使用 `recipe` 约束，不要把清流/温蒂等产线体系硬绑到固定 `room_id`。

### 4.3 与现有文件的关系

| 文件 | 角色 |
|------|------|
| `data/base_systems.json` | System 目录：`tier`（`cross_station` / `same_station`）、priority、`exclusive_group`、slots |
| `data/trade_segments.json` | producer 条件 + consumer → `shortcut_id`；`roles` 声明贸易核心优先 fallback 链 |
| `data/trade_shortcuts.json` | L3 组合级 trade%/gold% 锚点 |
| `data/orchestration_rules.json` | 当前声明式 rule / alternative / role / relation 真源；执行器不按体系名分派 |

### 4.3 贸易 meta：跨站体系、同站锚点与核心优先

来源：`MECHANICS_REGISTRY.csv` 中「当与 X 在同一个贸易站」+ 公孙工具人表。
`base_systems.json` 保留跨站体系和历史同站锚点；当前 `assign_shift` 主路径会跳过 `witch_long_beta`、`blackkey_closure`、企鹅、推王等低优先 registry 抢站条目，贸易余站改由 `data/trade_segments.json` 的 `roles` 执行核心优先。

> **当前实现（2026-08-19）**：`meta_vina`、`vina_lungmen` 选型和按 producer 分别重跑的前缀已删除。八幡海铃、戴菲恩和涤火杰西卡由 `producer_rules.json` 登记，并在同一次 producer presence 子集枚举中比较完整生产前缀；凛御银灰规则仅保留 consumer 阈值事实，自动中枢 admission 为 `excluded_automatic`。房内完整响应行 joint/Bake 仍由 [DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md) 继续跟踪。

#### 已落地的数据与运行时归属

| id / role | 类型 | L3 shortcut | 当前运行时 |
|-----------|------|-------------|------------|
| 多贸但书 required anchor + 叙拉古动态注入 | 条件硬锚点 + 自然中枢/贸易候选 | `gsl_docus_syracusa` / `gsl_docus_solo` | 多贸龙门币布局先预留但书本人；八幡海铃、伺夜、贝洛内均不固定，伺夜/贝洛内不要求同站，也不要求入编 |
| `closure` | 可露希尔核心优先 | `gsl_blackkey_closure` / `gsl_closure_*` | 强制包含可露希尔；多贸中排在但书之后、完整龙巫之前，并优先可用的最高等级金单站；优先黑键可露锚点，缺黑键仍保留可露 |
| `witch` / `witch_fallback` | 龙巫 / 巫恋兜底 | `gsl_witch_*` | `witch` 强制包含精二巫恋 + 已持有龙舌兰；龙舌兰 `tier_up` 优先 β/α，`tier_0` 允许 β/α/白板三档 `gsl_witch_long0_*`；无龙舌兰时 `witch_fallback` 低于推王组，只做巫恋兜底 |
| `ling_jie_karlan` | control producer + L1 自然搜索 | `gsl_ling_jie_yaxin` 仅参考 | 只认领灵知 E2 中枢；精1孑由贸易搜索注入 |
| 格拉斯哥自然候选 / `penguin_*` | 普通贸易候选 / 企鹅 segment | `gsl_vina_lungmen` / `gsl_penguin_*` | Vina shortcut 只结算实际组合，不参与选型；企鹅仍按独立 role policy 处理 |
| `rosemary_perception` | 声明式跨设施 Rule + global resource gate | — | 核心、路径成员与实际 producer 先形成临时 assignment；`resolve_base` 得到感知 ≥50 后才提交 plan |

#### 贸易核心 role 顺序

贸易金单余站按 `pick_trade_meta_then_plain` 尝试：

1. 一贸龙门币布局由可露希尔 required anchor 先占位；多贸龙门币布局由但书 required anchor 先占位。核心所在房的其余位置按 `total_efficiency` 搜索；`gsl_docus_solo` / `gsl_docus_syracusa` 只结算实际组合。
2. 默认多贸龙门币的第二核心是 `closure`：`gsl_blackkey_closure` 优先；否则 `gsl_closure_*`；否则包含可露希尔的最高可用组合。可露希尔优先进入剩余的最高等级金单站。
3. 默认第三核心是 `witch`：必须同时包含精二巫恋、已持有龙舌兰并进入三级金单站；龙舌兰 `tier_up` 优先裁缝 β/α，`tier_0` 允许 β/α/白板三档 `gsl_witch_long0_*`，其他普通 blank shortcut 不进入自动 role。
4. `witch_fallback` / `karlan` / `penguin` / plain：无龙舌兰巫恋兜底、灵知孑、企鹅、散件工具人三人组，且排除黑键与巫恋同房冲突。格拉斯哥成员作为普通合法候选参与实际 `total_efficiency` 搜索。

Rosemary/黑键体系 alternative 激活时，在选定 alternative 的同一 Plan 决策中把双贸易站
peak 固定为但书 A + 可露希尔/黑键 B，并让完整龙巫进入 γ reserve。该特殊 packing 增加
黑键同房和 reserve 约束，但不改变默认 `docus → closure → witch` 顺序，也不能由已经填出的
房间反推体系激活。

这条顺序表达独立 core / fallback 策略，不是任意固定三人组优先级。八幡海铃、戴菲恩作为可选中枢 producer；凛御银灰不参与自动中枢 admission，叙拉古、格拉斯哥、谢拉格 consumer 的实际组合仍由合法候选和 solver 决定。伺夜与贝洛内不要求同站，八幡与单个贸易消费者的极端组合同样合法。

#### Phase 2 待建（贸易 bond）

| System id | fill_mode | 同房 bond（CSV） | tier / 备注 | L3 shortcut |
|-----------|-----------|------------------|-------------|-------------|
| `penguin_texlap_e0` | bond | 德克萨斯 + 拉普兰德（恩怨 +65% 贸） | vault 确认德克萨斯 E2 不失去恩怨；不再限制 E0-only，id 保留兼容 | `gsl_penguin_texlap_e0` ✅ |
| `penguin_texangel_e2` | bond | **德克萨斯 E2** + 能天使（默契） | 德须 **精2**；第三人 `pick_one` | `gsl_penguin_texangel_e2` ✅ |
| `penguin_exusiai_lemuen` | bond | 能天使 + 蕾缪安 E2（相伴 +25%） | 第三人 `pick_one` | `gsl_penguin_exusiai_lemuen` ✅ |

**企鹅物流注意**：上是 **两套核**（德狼 vs 能蕾），不是一条 System；243 双贸通常只认领其一。选型用 `exclusive_group: penguin_meta` 或 priority，**不靠 search 发现**。  
德狼路线以 vault 为准：德克萨斯 E2 不会失去「恩怨」，因此德狼不再限制 E0-only；`penguin_texlap_e0` / `gsl_penguin_texlap_e0` 名称暂保留为兼容旧 id。

#### 一般不表化为 System（仍由 Plain search / tag 处理）

| 机制 | CSV | 原因 |
|------|-----|------|
| 新约能天使「同城加急单」 | 同站每名拉特兰 +15% | tag 叠层，非固定二人 bond |
| 维娜「外贸决议」 | 同站 GSG 干员 +10% | L1 tag 搜索自然结算；Vina shortcut 只结算实际组合，不再作为戴菲恩 producer-gated 选型入口 |
| 孑「市井之道」 | 站内含义依赖订单上限与技能顺序 | 灵知线由 L1 搜索自然上浮，非固定 trade slot / active L3 |

当前 producer 前提（跨房，非 global pool）：

- 叙拉古：八幡海铃 E2 的动态贸易标签倍率；仅在八幡与实际叙拉古贸易成员自然同时入选时生效，L3 链段只读取稳定 source buff capability
- 喀兰：`karlan_precision`（灵知 E2）
- 格拉斯哥：戴菲恩逐贸易房 Glasgow `+10%/人`，不激活固定贸易 role

### 4.4 制造同房 bond（自动化链配套，Phase 2+）

`MECHANICS_REGISTRY.csv` 中「在同一个制造站」；与贸易 meta 并列登记，避免只改贸易漏制造。

| 组合 | CSV 技能 | 编排建议 | 状态 |
|------|----------|----------|------|
| 水月 E2 + 同房标准化技能 | 意识协议 / 标准化 α/β | 不注册体系；进入普通制造候选池，由 solver 结算同房技能并按 `final_efficiency` 自然搜索 | ✅ 普通制造搜索 |
| 阿兰娜 E2 + 温米 | 「搭把手！」 | 自动化金线固定 slot 或 `bond` + 第三人贪心 | ⚠️ shortcut 待补；registry 延至 Phase 3（避免抢公孙金线制造位） |
| Miss.Christine E2 + 酒神 | 盛餐的回报 |  niche；低优先 | ❌ |
| 怒潮凛冬 E2 + 乌萨斯学生自治团 | 情同手足 | tag 同房加成；可并进乌萨斯制造 meta | ❌ |

### 4.5 跨设施落位前提（非同房 bond，但影响 Plan）

| 干员 | 条件 | 影响 |
|------|------|------|
| 烈夏 E2 | **古米在贸易站**（患难拍档） | 制造站选人时须保留贸易古米位 |
| 清流 E1 | 每贸易站 → 当前制造站金 +20% | 自动化组已含；非 bond |
| 戴菲恩 E2 | 中枢 producer | 逐房 Glasgow 自然候选；winner 只绑定实际贡献者 |

---

## 5. 实施阶段

### Phase 0 — 模块边界 + Plan 类型

- [x] 新建 `layout/orchestrate/`，定义 `AssignmentPlan` / `SlotFill`
- [x] `assign_shift` 调用 `build_plan` → `execute_plan`（行为可先等价迁移）
- [x] **验收**：243 E2 现有集成测仍绿

### Phase 1 — System 选型（select，历史执行记录）

- [x] 合并 `claim_base_systems` → `select.rs`（`select_registry_systems`）
- [x] 支持 `exclusive_group`、德克萨斯 E0/E2 企鹅物流 tier 分叉
- [x] 迷迭香、自动化、红松、莱茵与贸易核心迁移为有限 alternatives；禁止全编制笛卡尔积和 `DailyTotals` 匿名跨域裁决

### Phase 2 — 组合数据

- [x] 补 `trade_shortcuts.json` / `trade_segments.json` / `shortcut.rs` consumer
- [x] §4.3 贸易 meta 待建行齐套（企鹅三路 + 推王 shortcut + 黑键 segment）
- [ ] §4.4 制造 bond：阿兰娜+温米（延至 Phase 3 `bond` execute，避免破坏公孙金线回归）
- [x] **验收**：每个 System 一条 golden test（plan → assignment 快照 + 预期 `shortcut_id`）

### Phase 3 — Executor（execute）

- [x] registry `fixed` / `bond` 落位（`execute_plan`）；贸易余站 `assign_trade_remainder` 贪心
- [x] **已移除 / 不再存在**：`apply_blackkey_colocate_rule`、`assign_trade_meta`、`complete_trade_anchor_rooms`（旧黑键贸锚）
- [x] 贸易核心 role：`docus` / `closure` / `witch` / `witch_fallback` 写入 `trade_segments.json`，由 `search/role_pick.rs` 统一执行
- [x] `assign_shift` 主路径跳过 `witch_long_beta`、`blackkey_closure`、企鹅、推王等旧 registry 早占站条目，改由 role policy 选择
- [x] **验收**：缺伺夜/贝洛内时但书仍进站；缺黑键时可露仍进站；缺裁缝 β 时龙巫只允许裁缝 α，否则不启用自动龙巫站

#### 巫恋 role

- **核心**：`witch` 是龙巫，强制包含精二巫恋 + 已持有龙舌兰；龙舌兰 `tier_0` / `tier_up` 均可进入排班 admission。
- **fallback**：自动龙巫内部仅裁缝 β → 裁缝 α；`gsl_witch_long_blank` 只保留单站结算兼容。
- **兜底**：无龙舌兰时走 `witch_fallback`，只强制包含巫恋；格拉斯哥不占独立 role 顺序。
- **编排**：不再把 `witch_long_beta` 当固定三人组早占站；由 role policy 在贸易余站搜索里强制包含龙巫锚点。

### Phase 4 — global effect 收拢（与编排并行）

- [x] 感知 producer（爱丽丝/车尔尼/絮雨）`scope: global` atom → `cross_facility`
- [x] 删 `resolve.rs` 的 `apply_perception_producers` 硬编码
- [x] 制造房 `room_layout_for_manu` 声明式扣回 scope=Global atom
- [x] Rosemary rule 把实际 producer 解析到临时 assignment，再由统一 `resolve_base` 执行感知 gate；无专用 producer evaluator
- [ ] `skill_table` 其余干员 atom 标 `scope: global`（乌有人间烟火等）
- [ ] 删 `resolve.rs` 其余 `apply_*` 硬编码
- [ ] `GlobalInject` 留 `control/interpreter`，供 segment producer 读取
- [ ] 见 [INTERNAL/CROSS_FACILITY.md](INTERNAL/CROSS_FACILITY.md)、[EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) §九
- [x] 迷迭香 `shift_bind`：`schedule/shift_bind.rs` + `team_rotation`（见 [SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md)）

### Phase 5 — team-rotation 对齐

- [x] α/β 从 peak 切半保留编排已认领贸易 core；γ 走 `assign_team_gamma_half`，不含 `meta_vina` role
- [x] `assign_shift_with_plan` + `TeamRotationReport.peak_plan` 供轮换层只读编排计划

---

## 6. 控 bug 策略

1. **分层验证选型边界**：高优先硬规则单测验证结构不变量，且不靠 solve 决定 required core；late competitive 可调用对应 domain solver，验证候选相对普通基线的 Pareto 比较。
2. **以通用不变量矩阵为主**：覆盖 gate、role、cardinality、room-order、backtracking、bind 等 schema 语义，并保留关键体系端到端回归；不要求每个体系拥有专用实现函数。
3. 端到端：`layout team-rotation` + [fixtures/243](../data/fixtures/243/README.md)。
4. 迁移期可用环境变量/feature 对比旧 `assign_shift` 总分（短期）。

---

## 7. 明确不做

- 用 `C(n,3)` + solve 分「发现」已知 meta 组合。
- 在编排层重复实现 global pool 逻辑（归 `cross_facility`）。
- 新 repo / 全量 v3 重写 L1–L3（机制回归成本过高）。
- `orchestration_engine_design.md` 中的全编制笛卡尔积 + 评分选优（组合已表化）。

---

## 8. 相关文档

| 文档 | 内容 |
|------|------|
| [BASE_ASSIGNMENT.md](BASE_ASSIGNMENT.md) | 现行单班编制流水线 |
| [SCHEDULE_ROTATION.md](SCHEDULE_ROTATION.md) | αβγ ABC 轮换、shift_bind、现行排班入口 |
| [SYSTEM_CHAINS.md](SYSTEM_CHAINS.md) | 迷迭香/自动化/推王等体系链参考 |
| [INTERNAL/SHORTCUT_MATCHING.md](INTERNAL/SHORTCUT_MATCHING.md) | L3 匹配与 segment 注册表 |
| [INTERNAL/CROSS_FACILITY.md](INTERNAL/CROSS_FACILITY.md) | global atom 编排（resolve 内） |
| [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) §九 | AtomScope Room vs Global |

---

## 9. Agent 协作提示

- **改贸易 meta 进编** → 先读本文 + 改 `base_systems.json` / `trade_segments.json`，**不要**先改 `search/trade.rs` 打分。新增 System 时务必填写 `"tier"` 字段（`cross_station` / `same_station`）。
- **改机制数值** → 仍走 [AGENTS.md 的“核心分层边界”](../AGENTS.md#4-核心分层边界)（L1/L2/L3）。
- **改 global 池** → `cross_facility/` + `scope: global` atom，不动 assign。
- **验证模拟** → [Debug 指南的“复现入口”](MAINTENANCE_MODE.md#22-复现入口)，使用 `plan` 或 `layout team-rotation`。
