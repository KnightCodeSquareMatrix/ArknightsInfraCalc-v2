# 设施无关条件化响应 Bake 实施计划

> 文档角色：active-change
> 生命周期状态：ready-on-request
> 当前真源：docs/QUALITY_AND_AUDIT.md；docs/ORCHESTRATION_LAYER.md；docs/SCORING_MODEL.md
> 摘要：设施无关条件化响应 Bake 的后续实施任务

> 历史进度：2026-07-17 曾获用户授权并推进；当前没有活跃 writer，等待用户重新授权，或
> 未来确认产品预算后出现违反时恢复。
> 性能口径：离线条件响应用于让 cache miss 只变慢、不换答案；“标准全精二 243 warm
> `team-rotation` 200ms”是历史愿景，尚未由用户确认为当前预算，不能作为本轮已触发的硬承诺。
> 当前样例：producer full-E2 debug rotation 约 8.4s，不与旧 warm/release 样例直接比较，也不
> 表示当前性能已验收。
> 关联计划：[动态 Producer A+](DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。本文只负责
> Bake 的物化边界、生成流程和运行时查询，不重新定义 producer 业务规则与 comparator。
> 贸易是首个 vertical slice，制造是第二设施通用性验收，不复制平行 pipeline。

## 当前交接：首批贸易 room-local 物化

> 交接状态：2026-07-17 已完成真实技能提取、用户裁决和首批 schema v12 贸易 room-local
> 机制物化；自动联合选型仍由动态 producer TODO 跟踪。制造、L2、跨房与 runtime Join 尚未
> 纳入本批，不预设按单房、半区或全基地完成了全部 Bake。

### 用户最新方向

Bake 不应为每个干员或完整全局编制穷举响应，而应只物化**无法由单个干员独立确定的最小
机制组合**。例如格拉斯哥链应从戴菲恩、推进之王、摩根、维娜·维多利亚的真实技能出发，
判断哪些最小有效子集会产生不同响应；不能因为常见满组胜出，就只 Bake 满组或把它写成
required admission。

只读提取阶段已完成以下范围；其后首批实施已修改 Bake schema：

1. [x] 完整读取 `data/skill_table.json` 中贸易、制造、中枢和全局资源相关技能。
2. [x] 用 `data/operator_instances.json` 解析每个 buff 的实际 owner、tier、设施和 tag。
3. [x] 核对 L2/L3 owner：`trade/interpreter.rs`、`trade/gold_flow.rs`、
   `trade/order_mechanic.rs`、shortcut/segment registry，以及制造 interpreter。
4. [x] 将技能分为“单人可独立结算”和“必须联合确定”；只对后者列最小机制闭包。
5. [x] 对每个闭包列出响应不同的有效子集或阈值、作用域、仍需运行时提供的状态，
   以及普通队友是否可留到运行时 Join。
6. [x] 用户确认首批组合和戴菲恩逐房作用域；2026-07-27 又裁决完整 prefix 使用可参数化的
   贸易/制造加权效率 comparator，当前权重 `1:1`。

建议输出表：

| 机制 | owner / buff | 最小角色集合 | 响应不同的子集或阈值 | 外部状态 | 普通队友处理 |
|---|---|---|---|---|---|
| 格拉斯哥链 | 戴菲恩 + 实际格拉斯哥 consumer | 见下文逐房 consumer 与 producer 两个闭包 | `n_g(r)=0/1/2/3`，推进之王 presence；不能预填满组 | 同班、各贸易房实际 tag 数 | 不改变观察量者运行时 Join |

### 提取口径

以下“最小角色集合”表示产生非单人响应所需的最小机制观察量，不是 required admission，
也不表示必须把列出的角色固定同房。两个不同 logical operator mask 即使响应相同，仍保留各自
候选，只允许共享 response。普通队友只有在不改变该闭包的 tag、partner、skill family、
settled efficiency、limit contribution、L2 tag 或全局资源观察量时，才可作为白板 runtime Join。

给定房间、订单、布局和全局状态后，固定效率/上限、配方门控、设施等级/数量、全基建静态
人数、固定 mood/ramp 等技能均可由单个 CandidateRow 独立结算，不建立联合角色闭包。单人
携带的订单 tag 虽不要求搭档，但多个 tag 共存时必须由同一个 L2 订单机制闭包联合确定。

### 贸易房内与贸易 L2/L3 闭包

| 机制 | owner / buff | 最小角色集合或观察量 | 响应不同的子集或阈值 | 作用域 / 外部状态 | 普通队友处理 |
|---|---|---|---|---|---|
| 格拉斯哥本房 consumer | 维娜·维多利亚 `trade_ord_spd&par[001]`；摩根 `trade_ord_spd_par[000]`；推进之王为 partner | owner + 本房 `cc.g.glasgow` count；摩根另读推进之王 presence | 维娜：其他格拉斯哥 `0/≥1`；摩根自身计入 selector，可达 `n_g=1/2/3` × 推进之王 `0/1` | 同房；无外部状态 | 非格拉斯哥且非推进之王者可 Join；否则更新 signature |
| 企鹅物流 partner / segment | 德克萨斯、拉普兰德 buff 与能天使等 partner；现有 segment registry | 每个显式 partner 二元关系；L3 另有精确组合匹配 | partner absent/present；L3 命中/不命中 | 同房；segment 是现有实现路径，不是 required group | 第三人可 Join，但命中 L3 时解释路径会改变 |
| 贝洛内—伺夜 | 贝洛内 `trade_ord_limit&cost_P[020]`、`trade_ord_spd_ext[020|021]`；伺夜为 partner | 贝洛内 + 伺夜同房 presence + 伺夜全基地 presence | 不在基地 / 在基地不同房 / 同房 | 同房上限 + 全基地效率 | 无关房友可 Join；不能把两个 presence bit 合并 |
| 深巡、赫德雷的点名 presence | 深巡 `trade_ord_spd_ext[000|001]`；赫德雷 `trade_ord_par&per[000|001]` | owner + 被点名角色的基地 presence mask | 深巡二元；赫德雷为伊内丝/W 的四种 mask | 全基地 presence，不要求同房 | 房内普通队友可 Join |
| 巫恋 peer absorb + 裁缝 | 巫恋 `trade_ord_vodfox[000]`；裁缝 α/β tag owners | 巫恋 + peer count + 两个 peer 的订单 tag/limit/state profile | peer `0/1/2`，每人使巫恋 +45%；无 tag / α / β，β 优先 | 同房 + L2 order mechanic；订单类型和基础交付 | 任意新增 peer 都更新 response；被吸收者的订单 tag、limit 和状态仍须保留 |
| 订单 tag 优先级 | 但书 breach、龙舌兰 investment、可露希尔 closure、U-Official eureka、佩佩 exclusive、裁缝 α/β | 同房 L1 产出的完整订单 tag mask | closure → eureka → pepe → regular distribution；investment 读取基础 delivery，且与 breach 互斥 | 同房 L1→L2；订单种类、delivery、总效率 | 不携带 tag 且不改 delivery 者可 Join |
| 赤金有序链 | `gold_flow.rs` 登记的绮良、图耶、鸿雪等 role buff | CandidateRow 中有序 gold-flow role 序列 | 角色顺序、tier role、订单类型不同均可能改变响应 | 同房有序 L2；真实赤金线、初始虚拟线、杜林虚拟线 | 无 gold-flow role 者可 Join；role owner 不能按无序 mask 合并 |
| 同房效率/上限反馈 | 孑 `trade_ord_limit_count[000]`；雪雉 `trade_ord_spd_variable2[...]`；锏 `trade_ord_spd_variable3[000]`；琳琅诗怀雅 `trade_ord_spd_variable[000]` | owner + peers 的 settled-eff / limit-contribution profile | 按各公式 bucket；不是单纯人数 | 同房强顺序 L1；订单数、基础上限 | 可 Join，但 peer profile 改变时必须重查 |
| peer-count | 火哨 `trade_ord_spd&share[000]`；吉星 `...[001|002]` | owner + peer count | `0/1/2` | 同房 | 任意加入一人都更新 count |
| room tag-count | 焰狐龙梓兰 `trade_ord_orchd2[000]`；新约能天使 `trade_ord_spd_par[001]` | owner + 对应 `cc.g.snhunt` / `cc.g.laterano` count | owner 自身带目标 tag 时可达 `1/2/3`；其他 tier 按实际 owner/tag 映射编译 | 同房 | 仅相关 tag 队友改变响应 |

### 制造房内闭包

| 机制 | owner / buff | 最小角色集合或观察量 | 响应不同的子集或阈值 | 作用域 / 外部状态 | 普通队友处理 |
|---|---|---|---|---|---|
| 红云 / 泡泡仓容反馈 | 红云 `manu_prod_spd_variable[000]`；泡泡 `manu_prod_spd_variable3[000]` | owner + 全部同房 limit contributors | 红云读贡献总和；泡泡按每人贡献 `≤16/>16`；同房时泡泡路径覆盖红云 | 同房 L1 phase；配方、limit atoms | 带仓容贡献者不能当白板 |
| 槐琥团队精神 | 槐琥 `manu_prod_spd_variable2[000]` | 槐琥 + peers 的 `skill_eff` profile | `floor(peer_skill_eff_sum/5) × 5`，cap 40 | 同房；配方影响 peer skill-eff | peer 跨 bucket 时更新 signature |
| 水月标准化 / 海沫兼容 | 水月 `manu_skill_spd1[000]`；海沫 `manu_skill_change[000]` | 水月 + 海沫 presence + standard count；海沫存在时将 Rhine/Pinecone family 并入同一个总数 | 海沫 absent/present；合并后的总 count `0..3` | 同房 buff family count | 不带相关 family buff 者可 Join |
| 莱茵计数 | 多萝西 `manu_skill_spd1[010]`；溯光星源 `manu_skill_limit[000]`；娜斯提 `manu_formula_spd&cost_bd[100]` | 前两者读同房 Rhine family；娜斯提读全基地 Rhine workforce | 同房 count `0..3`；全基地 count `0..5`、cap 15 | 同房与全基地是两个独立统计量 | Rhine 角色更新相应 signature |
| 阿兰娜 partner / platform | 阿兰娜 `manu_prod_spd_double[000]`、`manu_token_prod_spd[010]`；温米为 partner | 阿兰娜 + 温米 presence + power platform count | partner `0/1`；平台数量多值 | 同房 partner + 布局 | 无关房友可 Join |
| peer absorb / 自动化 | 冬时 `manu_prod_spd&manu[...]`；温蒂、异客、掠风等 `manu_prod_spd&power[...]` | 具体 owner/tier buff + peer count/profile + 有效 power station count | buff family/tier、房间人数和 power count 均可能改变响应，不能只压成 absorb presence | 同房 + 布局；当前 live 仅三个 `power[...]` buff 会禁用深海猎人中枢加成，冬时不会 | peer skill-eff 归零后仍保留 layout-eff、limit、tag、state、mood |

### 中枢动态注入与跨房闭包

| 机制 | owner / buff | 最小角色集合或观察量 | 响应不同的子集或阈值 | 作用域 / 外部状态 | 普通队友处理 |
|---|---|---|---|---|---|
| 戴菲恩—格拉斯哥 producer | 戴菲恩 `control_tra_limit&spd[010]` + 各房格拉斯哥 workforce | 戴菲恩 presence + 每房 `n_g(r)`；与本房 consumer signature 联合 | 每房 `n_g(r)=0/1/2/3` | 中枢→当前贸易房 | 非格拉斯哥者可 Join；格拉斯哥者更新逐房摘要 |
| 凛御银灰—喀兰阈值 | 凛御银灰 `control_tra_limit&spd3[000]` + 喀兰 workforce | producer presence + 每房 `n_k(r)<3/≥3` | 每房二元阈值；最终读达标房数 | 中枢→贸易跨房 | 只有跨过阈值的喀兰 Join 改变响应 |
| 八幡海铃—叙拉古逐房注入 | 八幡海铃 `control_tra_limit&spd2[000]` + 各房叙拉古 workforce | producer presence + 每房 `n_s(r)` | 每房 `0..3` | 中枢→当前贸易房 | 当前贸易房叙拉古 Join 更新逐房摘要 |
| 歌蕾蒂娅—深海猎人 | 中枢歌蕾蒂娅 + 制造 `cc.g.abyssal` workforce | producer presence + 本房 count × 全制造 count + `manu_prod_spd&power[000|010|020]` presence | 本房 `0..3`；全制造 `0..全部制造席位`；结果 cap 45/90 | 中枢→制造跨房；当前 live 只以三个 power buff 禁用该加成 | 深海 tag 或指定 power buff 改变响应 |
| 涤火杰西卡—黑钢总数 | 涤火杰西卡 `control_bd_spd[000]` + 制造 `cc.g.blacksteel` workforce | producer presence + 全制造黑钢总数 | `0..全部制造席位`，每人 +5 | 中枢→全部制造房 | 黑钢 Join 更新总数；不建立固定 team/bind |
| 怪猎 peer-tag inject | 火龙S黑角 `control_token_tra_spd[000]`；麒麟R夜刀 `control_token_prod_spd2[000]` | owner + 中枢 `cc.g.monhun` peer presence | owner 单独 / 有 peer | 中枢同房→全贸易或制造 | 仅怪猎 tag peer 改变 inject |
| 其他门槛 inject | 斩业星熊 `control_token_prod_spd3[000]`；布丁 `control_token_prod_spd[000]`；森蚺 `control_pow_bot[000]` | owner + LGD peer / platform count / Lancet-2 power presence | peer `0/1`；platform `<2/≥2`；Lancet-2 `0/1` | 中枢同房或跨电站 | 仅相关 tag、平台、power workforce 改变响应 |
| 烈夏—古米 | 烈夏 `manu_formula_spd_P[000]` + 古米贸易 workforce | 战斗记录配方下，烈夏 + 古米是否在任意贸易房 | `0/1` | 制造→贸易跨设施；recipe gate 属于 CandidateRow / 房间上下文 | 制造普通房友可 Join |

### 全局资源 producer / consumer 闭包

| 机制 | owner / buff | 最小角色集合或观察量 | 响应不同的子集或阈值 | 作用域 / 外部状态 | 普通队友处理 |
|---|---|---|---|---|---|
| 感知转换链 | 黑键贸易、迷迭香制造 consumer + `Perception` producers/converters | consumer + 转换后的 `SilentEcho` / `ThoughtChainRing` 值 | 黑键按 `/4` 或 `/2`；迷迭香按 `/2` 或 `/1` floor bucket | 宿舍/中枢/global state→房间 | 不读写相关资源者可 Join |
| 人间烟火链 | 乌有贸易、截云/黍/铎铃等 consumer + 令、夕、重岳等 producer | consumer + `HumanFireworks` 可达值 | 各 consumer 按 `/5`、`/3`、`/10` 等 floor bucket | 中枢/宿舍/全基地计数→贸易或制造 | producer、consumer 或影响资源者更新 signature |
| 木天蓼链 | 火龙S黑角、麒麟R夜刀 producer + 泰拉大陆调查团 consumer | producer activation + `Matatabi` 值 + consumer | 怪猎 peer absent/present；资源阶梯 | 中枢/global state→贸易或制造 | 怪猎 tag peer 不能当白板 |
| 魔物料理链 | 森西宿舍 producer + 齐尔查克贸易 / 玛露西尔制造 consumer | `MonsterCuisine` 可达值 + consumer | 当前整数可达域按 `/1` floor 后的值分桶；若值域扩展则按 floor 等价类共享 | 宿舍/global state→贸易或制造 | 不影响资源者可 Join |
| 工程机器人链 | 至简 `manu_constrLv[000]` + `manu_prod_spd_bd[100|110]` | 至简 + `facility_level_sum_excl_meeting` 派生资源值 | consumer 按 `/16` 或 `/8` floor bucket | 全基地布局→制造 | 房友可 Join；布局值是外部 signature |
| Passion 链 | 初华、八幡海铃等 producer + 若叶睦 / 丰川祥子 consumer | 实际 producer roster + `Passion` 可达值 + consumer | 睦按 `/8`；祥子按 `/20` floor bucket | 中枢/宿舍 occupancy→贸易或制造 inject | producer/consumer 或 occupancy 变化更新 signature |

### 已裁决：戴菲恩作用域

用户于 2026-07-17 根据技能原文确认：戴菲恩进驻控制中枢时，同一贸易站中每有一名
格拉斯哥帮干员，该站订单获取效率 `+10%`。因此每间贸易房独立结算 `10 × n_g(r)`，不同
房间不得先求总和。代码修复将戴菲恩从 `tagged_count_in_trade_sum` 改为 current-room deferred
inject；八幡海铃同样按当前贸易房逐房结算，不再保留全贸易站总数 scope。2026-07-19 后 `meta_vina` 自动 role 已删除，
`vina_lungmen` 只保留实际组合 shortcut；assignment-aware control comparator 会按实际贸易房逐房
累计戴菲恩收益。完整条件响应行 joint/Bake 仍由本 TODO 与 A+ producer TODO 继续跟踪，当前
有序填房 policy 不宣称全局最优。

### 建议首批确认顺序

1. 纯同房计数：格拉斯哥本房 consumer、peer-count、room tag-count。
2. 制造通用性：红云/泡泡 limit contribution、槐琥 peer-eff bucket、水月/海沫 skill family。
3. 贸易 L2：订单 tag mask 及优先级；赤金链作为独立有序闭包。
4. 无语义冲突的动态 producer：凛御银灰逐房阈值、八幡海铃逐房注入。
5. 戴菲恩逐房效果修复通过后可纳入；全局资源链随后作为跨设施验收轴。

用户于 2026-07-17 确认上述首批范围和戴菲恩逐房 response。2026-07-27 用户进一步裁决：
完整 prefix winner 使用 `TradeManufactureWeightedEfficiencyV1`，当前权重 `1:1`；未来可通过
显式 policy 参数修改。首批实施不因此把常见满组、segment 或当前 winner 升级为 required
admission；各房 response 仍保留逐房分布，裸注入和不再决定最终 winner。

#### 首批贸易 room-local 物化（2026-07-17）

schema v12 已为现有完整贸易 CandidateRow 显式保存首批 room-local 机制签名：peer count、
Glasgow peer presence / exact count、推进之王 presence、`snhunt` count 和 `laterano` count。字段只在 row 的真实 atom
观察该状态时出现；无关普通组合不生成伪机制维度。不同 logical operator mask 均保留为独立
row，未使用 top-K、winner 或攻略组合裁剪；本批属于 exact row-derived materialization，不改变
候选集合、admission 或贸易 comparator。

完整 E2 贸易 catalog `out/bake-trade-room-mechanism-v11-20260717/` 的实测结果：

| 指标 | 数量 |
|---|---:|
| 全部贸易 rows | 134,324 |
| 携带首批机制签名的 rows | 30,306 |
| 不同首批机制字段签名 | 45 |
| 物理 room/order signatures | 6 |

这里的 45 是观察字段组合去重数，不是结构化 response dictionary 的去重数。维娜只保存
“其他 Glasgow 是否存在”的布尔值；摩根才保存 owner-inclusive 的 `1/2/3` 精确人数。
Catalog loader 和 verifier 都会对全部贸易 row 从实际 operators 重算机制签名，并继续对每个物理 signature 抽取
首/中/末结构化 response 与 live solver 差分。本批尚未把制造、贸易 L2、跨房 producer signature
或 runtime Join 接入该 key；这些输入仍由现有兼容门禁拒绝或 live fallback，Required 模式硬 miss。

schema v12 进一步记录 `combo_table.bin` bytes/hash 和逐 signature row count，并从当前 TradePool
重新枚举全部合法 operator identity set 与 catalog 精确对照。完整 trade-row adapter 只在该证明
通过后返回一次加载的只读 catalog handle；row identity 使用 `(signature_key, offset)`，不因是否
同时生成制造 catalog 而变化。

上述顺序只用于选择首批验证面，不是 top-K、hard constraint、required admission 或近似剪枝。
提取本身是 analysis-only；随后经用户裁决只修复戴菲恩已确定编制的 final resolve。候选集合、
排序、Plan 和 rotation 均未改变，自动联合选型仍按本 TODO 的既有范围继续实施。

### 已有运行证据

- 标准全精二 243 三班排班历史样例约 `2.1–2.5s`；`<=200ms` 是尚待用户确认的历史愿景，不是当前验收预算。
- 现有贸易 catalog 已在 `out/bake-trade-full-20260717/` 成功生成：134,324 行，生成约
  `0.68s`；18 条 live response 抽样和机制回归通过。
- 隔离数据目录 `out/bake-runtime-data-20260717/` 使用该贸易 catalog 后，三班排班仍约
  `2.0–2.4s`，说明单房贸易 Bake 不是主要收益点。
- profile 中名为“中枢-生产前缀候选”的 `575–825ms` 不是纯中枢成本，而是每个中枢前缀下
  的全局生产组合重复求值；不要据计时标签把问题简化成“Bake 中枢五人”。
- 单次制造阶段观测约 `146–174ms`，γ 替补和三班评分还会重复结算；真正目标是减少重复的
  全局机制组合求值，而不是只缓存一个普通制造三人组。

相关 evidence：

- `target/codex-runs/bake-trade-try-20260717/`
- `target/codex-runs/bake-runtime-try-20260717/`
- `target/codex-runs/manufacture-speed-check-20260717/`

### 禁止提前做出的简化

- 不从当前 top hit、常见攻略组或固定人名套餐推导 hard constraint。
- 不只 Bake 满组而删除单人、二人或三人有效子集。
- 不把同响应的不同人员 mask 删除；只允许共享 response。
- 不为普通白板干员逐人生成重复 Bake；普通队友优先保留为 CandidateRow/runtime Join。
- 不在尚未列清最小机制闭包前接管默认 `team-rotation`。
- miss 必须继续 live fallback；Required 模式仍用于证明完整命中。

### OpenCode subagent 交接

项目 Terra、Sol、Luna 已在 commit `47c69c0` 改为 Code Online：
`codesonline/gpt-5.6-terra|sol|luna`。OpenCode 配置不会热加载，下一位 AI 开始前应确认会话已
重启，再使用 Terra 做大范围技能调查、Luna 做稳定字段提取、Sol 做最终边界复审。

## 0. 2026-07-17 用户裁决与实施顺序

核心模型扩展为：

```text
CandidateRow
+ RelevantExternalStateSignature
+ RelevantCrossFacilitySummary
-> ConditionalResponse
```

外部名单必须按对目标 response 的完全等价状态归一化；不同 logical operator mask 即使响应
相同也只能共享 response，不能删除候选。状态空间通过机制依赖切片、真实可达签名、阈值/
封顶值域和 family 归一化压缩，不使用固定 top-K 或当前 winner 剪枝。

实施批次：

1. **验证控制面**：`BakeMode::Auto/Disabled/Required`；Required miss 硬失败；每次生成后
   严格校验 catalog 并运行现有机制回归；仓库正式生成入口随后运行完整 release test suite。
2. **依赖与规模编译**：建立设施无关 dependency slice，统计 CandidateRow、外部签名、摘要、
   response 数量、磁盘和内存；未测量前不冻结物理格式。
3. **贸易 vertical slice**：覆盖房间局部计数、跨房聚合和跨房阈值，强制 live/baked 差分。
4. **制造复用**：以 full-pool 和中枢到制造的外部影响验证公共 schema，不建第二套引擎。
5. **排班查询**：预排序 response 视图、operator-mask 精确 Join、peak/γ 共存签名和 winner
   完整 live resolve。
6. **发布与性能**：generation-id 临时目录、checksum、原子切换、失败保留旧 generation；
   用户确认预算后再设置标准 243 warm `team-rotation` p50 门槛，历史 200ms 愿景不自动生效。

依赖分析与规模报告阶段已完成；当前已进入首批贸易 room-local schema 物化。该物化不改变
候选集合、主路径 comparator、Plan admission 或 rotation。

### 2026-07-17 依赖编译阶段

已新增只读 `profile bake-dependencies` 入口。依赖编译器对当前全部 `Selector`、`Condition` 和
`Action` 使用穷举匹配；新增枚举若未分类会导致编译失败。分类先保守区分房内、同设施、
跨设施、全局布局和运行时状态。全局资源 producer 暂时指向 `global_resource` 中间节点，
下一步根据 registry 和实际 consumer 展开多目标传递闭包，不把来源设施误当成最终目标。

资源转换状态只包含“链关闭”和“provider + converter 同班完整激活”两类可达状态。缺一端的
半链不进入条件化 response 维度；该压缩来自已确认激活语义，不是按当前 winner 做启发式剪枝。
依赖报告同时输出 atom 的资源读写边和 conversion 的 provider/converter buff id。

首份报告中的具体 external 数量会随 Action 隐式依赖和全局资源边补全而更新；以命令生成的
最新 JSON 为准，不把早期 70 条观测作为冻结规模。
完整 JSON 产物用于下一步提取贸易/制造的最小充分签名和值域；本阶段尚未据此执行状态合并
或修改 Bake schema。

#### 资源 consumer 闭包与值域公式（2026-07-17）

`profile bake-dependencies` 现按 `(target_facility, response_field)` 输出资源反向闭包，并为
每条资源读取保存结构化 `floor/div/multiplier/step/cap` 公式。闭包从目标 response 的直接读取
出发，反向穿过 EffectAtom 的 producer/局部转换和 `CONVERSIONS` 的破坏性转换；registry
转换边保留 provider/converter buff 与同班激活条件。`GlobalInject*` 的资源读取归入其最终
影响的贸易/制造 efficiency 闭包，row 仍保留原始动作字段供诊断。该报告是 analysis-only，
不参与候选生成、排序、剪枝或运行时求解。

默认数据报告 `out/bake-resource-closure-20260717-final.json` 的当前规模：

| 闭包 | 直接资源 | 传递闭包资源 | 来源/转换边 |
|---|---:|---:|---:|
| 贸易效率 | 5 | 9 | 23 |
| 制造效率 | 7 | 11 | 26 |

全表共 8 个最终设施/response 闭包、28 条值域公式。贸易效率闭包包含
`silent_echo <- perception <- {dream, musical_section, memory_fragment}`；制造效率闭包包含
`thought_chain_ring <- perception <- {...}`。此处箭头表示从 response 向 producer 的反向遍历；
JSON edge 仍按 producer 到 consumer 的正向输出。制造闭包并显式包含 `virtual_power` 对
`PowerStationCount` 的整数截断/饱和影响。报告未发现资源自环或空闭包。

当前覆盖范围明确标为 `effect_atom_plus_global_conversion_registry`。贸易 L2 `gold_flow`
读取真实赤金线数、`virtual_gold_lines` 和杜林虚拟线数，但这些依赖不由 EffectAtom 表达，现作为
`unresolved_delegated_dependencies` 输出，不能在补上具名 L2 dependency contributor 前声称
贸易闭包完整。28 条公式目前均标记 `requires_producer_range_analysis=true`；下一步应结合
实际 producer、布局上界和同班 gate 求可达区间，再按断点生成有限等价类，不能直接枚举
理论整数全域或据当前 winner 缩减。

#### L2 contributor 与标准 243 场景域（2026-07-17）

`gold_flow` 现由其 owner 模块声明五类真实输入：`real_gold_lines`、
`virtual_gold_lines`、`durin_virtual_lines`、候选行内有序 gold-flow role，以及订单类型。
前三项进入外部状态签名；后两项已分别属于 CandidateRow 和 room signature，不重复进入外部
维度。`response_dependency` 只消费该声明，不再手写 gold-flow 资源名。

`profile bake-dependencies --layout data/layout/243_use_this_.json` 会将报告绑定到该蓝图场景：

- 物理赤金线固定为 `{2}`；
- 初始全局虚拟赤金线固定为 `{0}`；
- 杜林虚拟线由代码 cap 编译为 `{0,1,2,3,4}`；
- 三项均有有限 scenario domain，因此不再出现在 unresolved delegated dependencies。

这得到 7 个单变量等价桶，但尚未宣称完整外部签名只有 7 种，也未执行三维笛卡尔积。
gold-flow 是有序状态机，最终签名还必须与 CandidateRow 中的有序 role 联合验证。其余
EffectAtom 资源在当前报告中继续 `max=null`：owner cardinality、room-scope producer、同班 gate
和任意 `initial_global` 尚未统一编译。该保守状态禁止据此修改 Bake schema 或合并响应。

## 1. 核心模型

普通单房 Bake：

```text
RoomTeam -> RoomResult
```

本计划扩展为：

```text
RoomTeam
+ ControlEffectSignature
+ RoomFeatureSignature
+ CrossRoomSummary
-> BakedRoomResponse
```

这是单房 solver 的**条件化物化视图**。Bake 不保存完整中枢五人名单，也不保存完整多房
assignment；它只保存会改变目标房结算的充分状态。

运行时仍负责把多个单房响应连接成一个人员不冲突、跨房统计自洽的完整方案，并对最终
winner 执行一次真实 `resolve_base` 校验。

## 2. 为什么按效果签名 Bake

不同中枢组合可能对贸易结算完全等价。例如阿米娅、诗怀雅等不同名单最终都可能只产生
同族全贸易 `+7%`。若按五人名单 Bake，会重复生成大量完全相同的单房结果。

因此先把中枢组合投影为效果签名：

```text
ControlEffectSignature
  global_trade_flat
  active_deferred_rules
  karlan_precision
  named_trade_components
  semantic_version
```

两个中枢组合只有在对所有受支持单房 response 完全等价时，才能共享一个 signature。
心情、线索等不影响目标 solver 的差异不得进入贸易响应签名。

禁止使用 `package = vina_lungmen` 之类套餐名作为 key。签名必须来自已解析的 atom、
capability 和 resolved rule，不能重新引入 fixed package 选型。

## 3. 首期贸易维度

### 3.1 单房候选行

每个合法贸易房组合生成一条 `BakedTradeRow`：

```text
row_id
logical_operator_mask
variant_ids
room_level / capacity / order_kind
siracusa_count
glasgow_count
karlan_count
presence / shortcut capability
stable_tie_break_id
```

候选行不截固定 top-K。operbox、当前 `used`、plan anchor 和跨房姓名冲突在运行时过滤。

### 3.2 跨房摘要

首期摘要只保存三名 producer 真正需要的有限状态：

```text
siracusa_total
karlan_qualified_room_count
```

戴菲恩使用本房 row 自带的 `glasgow_count`，不把不同房间先求和。凛御银灰的达标数必须
由每房 `karlan_count >= 3` 推导。摘要值必须与最终选中 rows 重新核对，不能只相信查询 key。

### 3.3 响应内容

`BakedRoomResponse` 至少保存：

```text
row_id
control_signature_id
cross_room_summary_id
final_efficiency
unit_output
order_limit
mechanic_equivalent_efficiency
rule_id_id
breakdown_id
```

不得只保存一个匿名“组合生产力”。结构化字段用于 comparator、展示和最终 live 对账；
字符串、`rule_id` 与重复 breakdown 进入去重字典。

## 4. 离线生成

允许生成耗时数分钟到数十分钟。优化目标首先是完整性、确定性和可验证性，不以最短生成
时间为目标。

生成分为四步：

1. 枚举所有受支持 tier、房间等级、订单类型和合法单房成员组合，生成不可变 row。
2. 枚举由当前规则 registry 可达的去重中枢效果签名和有限跨房摘要。
3. 并行计算 `row x control signature x summary` 的真实 solver response。
4. 分片排序、归并、建索引、计算 checksum，完成后原子发布 generation。

每条 response 相互独立，适合使用 Rayon。worker 产生私有 chunk，避免争抢一个全局可变
`HashMap`；最后统一归并并分配稳定 id。

生成器必须记录：

- row、signature、summary 和 response 数量；
- 去重前后中枢组合/效果签名数量；
- 每阶段耗时、线程数、峰值内存和最终磁盘大小；
- solver 调用数、成功数、拒绝数与失败原因；
- 输入 hash、generator fingerprint 和 semantic model version。

## 5. 运行时查询

运行时流程：

```text
合法中枢组合 -> ControlEffectSignature
                          |
枚举有限 CrossRoomSummary |
                          v
各房预排序 BakedRoomResponse
          -> logical mask 互斥 Join
          -> operbox / tier / used / anchor 过滤
          -> 摘要自洽检查
          -> comparator 选择 winner
          -> 完整 assignment live resolve
          -> breakdown / rule_id 对账
          -> 一次 commit
```

八幡海铃和凛御银灰存在循环依赖：贸易成员决定摘要，摘要又改变各房排名。运行时按有限摘要
枚举情景，选出 rows 后检查实际摘要是否等于假设；不自洽的候选直接作废。

每个完整 prefix 的贸易与制造 response 分别汇总真实 `final_efficiency`，再由
`TradeManufactureWeightedEfficiencyV1` 比较；当前权重 `1:1`，未来调整值必须进入 catalog
semantic fingerprint。戴菲恩仍保留逐房 Glasgow signature，不因最终加权求和而先聚合 response。

## 6. 多线程与文件布局

建议按 room signature 和 control signature 分片：

```text
manifest.json
operators.bin
control_signatures.bin
cross_room_summaries.bin
trade_rows.bin
responses/<room-signature>/<control-signature>.bin
indexes.bin
dictionaries.bin
```

并行任务以 response 分片为所有权边界。每个分片写入 generation 临时目录；全部 bytes、
row count、index checksum 和 catalog hash 校验通过后再原子切换。读取方不得观察到新旧分片
混用。

运行时可以只映射当前 layout、room signature 和 control signature 所需分片，不要求一次
加载全部 response。

## 7. 明确不 Bake 的状态

即使离线时间充足，也不物化以下高维运行时状态：

- 完整中枢五人名单；
- 完整 operbox；
- 任意 `used` 子集；
- 完整多房 assignment；
- alpha/beta/gamma 班次排列；
- 不影响目标 solver 的心情、线索或展示字段；
- 所有设施人物的全局 `2^N` mask。

这些维度缺乏跨账号复用价值，或可由 logical mask 和运行时过滤廉价处理。允许多算不等于
保留完全等价的重复状态。

## 8. Cache gate 与后备

catalog 至少校验：

- schema、semantic model version、generator fingerprint；
- skill table、operator instances、shortcut、segment、standalone roster 和 producer registry；
- operator/variant dictionary；
- room signature、tier、order kind 和 Bake options；
- 每个分片的 bytes、hash、row count 和 index checksum。

任一 row、signature、variant 或 response miss 时，必须用同一规则现场调用真实 solver，并
继续进入同一运行时 Join。miss 只能增加耗时，不能缩小候选集合或改用旧 top-K 路径。

最终 winner 的 live resolve 若与 baked response 任一关键字段不一致，本次查询拒绝整个
catalog generation 并用 live reference 重跑，不能只修正 winner 分数。

## 9. 实施阶段

### Phase 0：规模测量

- 统计完整 trade rows、可达 control signatures 和 summaries；
- 用实际字段宽度估算 response 数、生成时间、磁盘和内存；
- 保存当前 Bake、`plan` 和 joint live reference 基线。

### Phase 1：稳定 row/signature schema

- 建立 operator/variant/signature 字典；
- 保证效果签名由通用 rule/capability 提取；
- 用等价与不等价中枢组合测试签名去重。

### Phase 2：并行 response 生成

- 用真实贸易 solver 生成完整结构化 response；
- 实现 chunk、归并、分片和 generation manifest；
- 不接入默认运行时。

### Phase 3：运行时 Join 与 live 对账

- operbox/used/mask 过滤；
- 摘要情景枚举与自洽检查；
- 多房互斥 Join；
- winner 完整 assignment live resolve。

### Phase 4：差分验证后启用

- 强制 live 与强制 Bake 在最小盒、full-E2、混合 tier、非标准 layout 上逐项一致；
- 注入损坏分片、旧 hash、未知 variant 和 response miss；
- 只有 winner、breakdown、`rule_id` 和 dependency 全部一致后才启用默认快路径。

## 10. 验收门槛

1. Bake 与 live 使用同一候选集合、规则 registry 和 comparator。
2. 三 producer 的 0/1/多 consumer、两两共存、三者共存和全部落败均有差分测试。
3. 戴菲恩 `(3,0)` 与 `(2,1)` 不混淆；八幡海铃逐房人数、凛御银灰逐站阈值均自洽。
4. 任意两个房间、任意中枢与房间之间 logical mask 无交集。
5. cache miss、损坏或不兼容只变慢，不换答案。
6. 最终 live resolve 与 baked `final_efficiency`、unit output、limit、breakdown、`rule_id`
   完全一致。
7. 记录完整生成耗时、线程数、响应数量、磁盘、内存、加载时间和运行时命中率。

## 11. 后续扩展

制造域可复用同一模型：

```text
ManuRoomTeam
+ ControlEffectSignature
+ RoomLocalFactionCount
+ CrossFactorySummary
-> BakedManuRoomResponse
```

涤火杰西卡是首个验证样例，但必须在贸易版稳定后单独审计并实施。灵知的负效率/订单上限
response 也可进入贸易 catalog，但必须先裁决其 comparator。不得因为 schema 能表达，就在
本计划中顺手改变尚未确认的业务选型。

## 12. 非目标

- Bake 完整全基建排班或班次组合；
- 用套餐名、干员名分支替代 effect signature；
- 用 Bake 结果替代最终完整 assignment resolve；
- 为减少生成时间使用固定 top-K 或近似剪枝；
- 在没有规模测量前承诺具体秒数、内存或磁盘上限；
- 同时重做贸易、制造、发电和轮换全部域。
