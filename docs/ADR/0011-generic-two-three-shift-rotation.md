# ADR 0011: 二/三班通用时长排班与泡泡火神全局强制

> 文档角色：decision
> 生命周期状态：accepted
> 当前真源：docs/定时换班.md；docs/排班模式.md；docs/泡泡火神源石优先.md
> 摘要：为普通二班/三班增加输入时长并共用终态收尾，在全部该范围内强制泡泡/火神源石预占

> 日期：2026-10-21
> 决策状态：accepted
> 实现状态：in-progress（历史源码基线完成过 release 与泛型代表/高持有 batch，不等同于本次 WIP 源码验收；历史 DeepSeek 阶段、rotation/MAA 与用户指定的 `gpt-6.1-sol` 独立复核指出仍缺源石预占局部失败的完整 Box 成功 fallback 及配对 gate 负例，`gpt-6.1-sol` 结论为 `CONTINUE`。当前 WIP 的新增 review findings 见 active-change checkpoint §6.9；配置的旧 `gpt-5.6-sol` reviewer 启动失败。该状态只指本 ADR 单元，不表示整个排班统一内核 active-change 完成）
> 范围依据：用户在 2026-10-21 会话中明确的二/三班泛型时长、四班特例暂缓及泡泡/火神全局强制决定
> 核验基线：源码基线 `7916d90`；历史 release evidence `target/codex-runs/generic-two-three-originium-completion-20261021/manifest.json`（不绑定当前工作区）

## 背景

现有输出类型已保存每班 `duration_hours`，内部也有以状态向量表达班次的 `RotationStateSpec`；但入口仍以封闭的 `TimedRotationProfile` 分派固定时长，ABC 构造使用固定长度时长数组，主备又走独立构造和 profile 收尾。`finalize_profile_rotation` 与 `finalize_timed_rotation` 责任重叠，MAA 有若干策略仍从 profile 名称或“2班”标签推导。

未提交的泡泡/火神预占改动覆盖 ABC 两个 profile，并向通用 layout assignment 传递冻结房间。用户随后将适用范围明确为全局强制，同时要求排班先接受泛型时长；因此不能把当前 ABC-only 入口或特例参数当成最终 owner。

本 ADR 同时偿还与本功能直接相关的架构债。它不授权清理无关 profile、机制、trade/Bake 或全仓代码。

## 决策

### D1. 普通二班/三班接受泛型时长

- 定时轮换的普通二班和三班接收有序班次时长序列；时长不再由固定的 `12/6/6`、`12/12/12`、`12/12` 或 `24/24` profile 名称决定。
- 时长序列保持调用方提供的顺序。普通泛型周期由调用方限制为 24h、36h 或 48h；班次总时长、每日加权产出、连续心情/ETA、班次间休息和 MAA 描述均消费同一序列中的实际时长。
- 调用方前端负责拒绝不合法的二班/三班组合，并将普通泛型周期限制为 24/36/48h。此轮不扩建后端业务校验；后端按收到的已验证输入求解。
- 现有命名时序仍须作为兼容输入映射到相同班次序列和原有策略。不能通过为每种新时长新增一个 profile 枚举来实现“泛型”。若 Worker 的闭集 wire 需要改变，使用兼容的新契约版本；不得静默改变旧版本含义。

### D2. 拓扑构造分开、终态收尾共用，不抽象四班专用语义

- 普通二班与三班保留各自的 topology-specific 周期构造；两条路径进入共同的完成/终态收尾 owner。策略层仍保留二队主备与 ABC 等真实不同的队伍/轮休拓扑；不能把“班次数”当作队伍拓扑，也不能把所有差异塞进新的通用 DSL。泛型 ABC 并列最长时休班相位放在较短班，尽量让 α 覆盖全部可覆盖的并列最长班；完全等长保留旧相位。
- 用户于 2026-10-23 明确：普通泛型 ABC 同样使用菲亚 N/A/B 候选层；候选按调用方实际班时和 α 相位独立完整求解，再比较完整周期价值。此策略层不合并主备与 ABC 的拓扑构造，也不改变普通主备或四班专用路径。
- 共同收尾 owner 负责消费已构造的实际时长，完成普通补位、周期加权、最终效率/心情结算、warnings 和最终 assignment 检查。MAA 只投影最终报告，不重新决定班次、房间或成员。
- 本轮不要求把两种 topology 的构造合并成统一周期执行器；后续若要删除重复构造或提取更深层执行器，必须另有行为等价证据，不得由本 ADR 推导已完成。
- 候选准入和终态复核可以分阶段执行，但最终“实际工作岗位、休息、bind/required presence”的语义只有一个终态 owner；宿舍 presence 不得被算成工作。
- 8/8/4/4 菲亚、深海 7/5/7/5 等四状态专用模式保持原路径、时长与输出，本轮不泛型化，也不因共用二/三班 owner 而改变行为。

### D3. 泡泡/火神在范围内全局强制

- 对本 ADR 范围内的所有普通二班/三班轮换，若泡泡至少 E1、火神 E2，且蓝图存在首个合法源石制造房，则无产品开关地建立强制预占；适用范围不再仅限 ABC。
- 房间选择沿用 `docs/泡泡火神源石优先.md` 当前口径：按蓝图顺序选首个合法的三级源石制造房（制造、源石配方、容量至少三）。不得按收益换房或扩成所有设施/所有候选比较。实现须确认蓝图容量与等级约束的实际关系，不能在代码和 canonical 间留下不一致。
- 泡泡与火神是同房 hard anchors，并按既有用户决定属于 α 队；α 应由通用拓扑放到最长时段。若有 12 小时或更长时段则优先覆盖该时段；没有 12 小时时也不得因此关闭强制预占。不得写死 `shift == 0`、某个固定班数下标或 `duration == 12` 来代替时长/队伍事实。若最长班并列，沿用既有确定性队伍/状态规则，不新增由单个房间或 fixture 决定的 tie-break。
- 两人按所选拓扑的现有轮休关系作为不可拆的组合：三队 ABC 保持两班工作、一班休息；二队主备跟随 α 主队与 β 备队状态，不把 ABC 的 2/1 计数硬套给二班。第三席继续使用普通合法制造候选；候选耗尽时保留 pair、短员并按既有 warning 口径报告，不把缺第三人升级成求解错误。
- 若资格或合法房不成立，规则不适用，执行原有排班且不产生本规则 warning。若满足条件但预占候选无法完成，正常产品路径保留可用的普通排班并给可定位 warning；显式 `--assert-invariants` 必须指出最终 assignment 未满足强制预占。这里的“全局强制”表示所有适用二/三班都必须尝试并优先满足，不表示把质量诊断升级为用户可见的 solver error。
- 冻结房身份应随同一最终周期事实传播到切半/owner 分配、普通补位、菲亚或其他适用后处理、终态检查和 MAA。不得让 generic layout pipeline 成为泡泡/火神产品策略的另一个 owner；通用设施层只消费明确的预占/anchor 事实。

### D4. 输出与保证

- MAA 必须和最终 `TeamRotationReport.shifts[].assignment` 一致；每班使用实际输入时长生成间隔和描述，不得从 profile 名称恢复时长或重选泡泡/火神。
- 保持现有可观察输出策略，尤其二班宿舍自动填充提示；若泛型输入使 `plan_times()` 不再能唯一表达策略，应传递显式产品策略，而不是通过时长字符串猜测。四班模式 MAA 不变。
- 固定生产候选、强制组合、完整性保证和近似搜索必须清楚分类。此 ADR 不宣称全局最优；现有受限搜索、top-K 或回退等级不因泛型化而被升级为完整搜索。
- 正常产品结果中的质量问题按 warning 保留可用排班；显式 invariant gate 是独立验收，不得把 warning 输出伪装成 gate 通过。

## 非目标

- 8/8/4/4、7/5/7/5 及其他四班特殊路径的统一、改时长或行为调整。
- 后端承担不合法班数/时长组合的前端校验职责。
- Mower 动态换班、自动换班新模式、实时宿舍恢复模拟或全局最优保证。
- 贸易/制造机制、效率排序、Bake、四赤金完整候选比较、普通静态 layout 源石策略。
- 全仓清理或与二/三班泛型执行链无关的 AI debt。

## 迁移与验收

实现前从实际调用链确认 CLI、Worker/plan.compute、配置模型、schedule、finalizer、MAA 和 `--assert-invariants` 的 owner；检查当前未提交源石预占 diff 并保留可复用行为，不能覆盖或将它误认为已交付。先声明一次完整的行为范围，然后由单一 writer 完成纵向修改。

既有具名时序的菲亚动作槽、宿舍自动填充提示、ABC 与主备队伍拓扑仍由各自 canonical 裁决。普通泛型每个超过 12h 的班次最多生成一次菲亚动作；动作仍受 ready、目标去重和最终 assignment 核对。24/24 named 与泛型长班均按实际 ETA/心情准入，取消姓名排除名单；泛型超过 24h 使用输入的实际班长，不继承 named 24/24 的姓名策略。

### 当前续接补充（2026-10-23）

普通泛型 ABC 已接入 N/A/B，与 named ABC 共用候选策略而继续保留实际时长和 α 相位；恢复输入顺序时移动整份班次结果。`fiammetta_enable=false` 时只运行 N，CLI/Worker 共用的 request-aware invariant gate 另核对最终没有菲亚动作。该源码单元经 fresh release、ABC/主备各 38 个 Box/配置 gate、H2 最长的真实动作见证、本地 Worker 和独立 `gpt-6.1-sol` 终审 `PASS`；证据与适用范围见 active-change §6.9 的本日 checkpoint。pair 局部预占失败的完整 Box 配对 witness 等本 ADR 其余证明仍未闭合，故本 ADR 保持 `in-progress`，不能将该局部 PASS 推广为整体交付。

至少用真实 Box 经过正常排班入口生成完整二班与三班结果、最终 assignment、warnings 和 MAA；覆盖旧 named timing 的行为基线、代表性非均匀时长（总周期 24/36/48h）、泛型菲亚动作、bubble/vulcan 合法/不适用/预占失败与短员边界、最长班/α归属、绑定和休息。使用 release 的 `--assert-invariants` 与 representative/high-owned 持久 Box gate，核对每班最终 assignment 和 MAA；Python 只调用 Rust gate，不复制规则。轮换/MAA 终态 assertion diff 须经适用的独立领域复核。不得新增或运行源码单元测试来替代真实 Box 入口与 invariant gate。

保留四班路径的旧结果作为明确不变边界；用旧 named 二/三班输入作差分，只允许用户已授权的时间/源石组合行为变化。文档生命周期、接口版本、索引/route 和 active-change checkpoint 同步后方可关闭本 ADR 对应的实现工作。

### 当前实施阶段（2026-10-21）

- **该源码基线的历史实现与验收**：共享工作区在基线 `7916d908192e3833ff1cf13e60c7bbebfcac9c29` 上的普通二/三班、Worker v7、泡泡/火神预占及关联文档修改，经 `cargo fmt --all` 与 fresh `cargo build --release -p infra-cli` 后由 release CLI 验收。泛型 ABC `14/10/12` 和泛型主备 `14/10` 各对 representative + high-owned 持久 Box 做 `--assert-invariants` Python batch，均为 38/38；eligible named 主备 `2` 的完整 Box gate 通过，generic ABC 普通 pair、generic 主备 30/18 pair、named 和泛型旧时序、三种总周期、输入最长班/并列最长班、菲亚、Worker v7 replay/v6 拒绝及 MAA projection 另有逐场景证据。fresh binary 和全部日志见 `target/codex-runs/generic-two-three-originium-completion-20261021/manifest.json`。该 evidence 绑定上述历史源码基线和 diff，不等同于当前共享工作区的重新验收。
- **独立复核状态**：DeepSeek stage review 为 `CONTINUE`，确认两处编译调用修复和已覆盖正常路径，但要求真实完整 Box 触发“pair 局部预占失败 → 普通峰值成功 + warning”，并在同场景显式 gate 失败。rotation/MAA review 确认导出按最终 assignment 与实际班时投影；仍未验证 MAA 执行端对“显式宿舍成员 + autofill”组合的实际解释，不把文件生成等同于游戏内执行。用户随后指定的 `gpt-6.1-sol` 独立复核未发现已证实回归，但因缺少 pair-fallback 见证返回 `CONTINUE`。前两次代表/高持有批次使用 named `3`，后续补跑的两个 generic topology batch 才是泛型批量覆盖。
- **未闭合边界**：`main-backup@30/18` 正常入口保留 pair 与完整排班但 warning；其显式 gate 因 `rotation_cycle_mood` 失败，而不是 pair 丢失，所以不能充当 pair 预占失败的普通/显式配对证据。eligible Box 的 ABC `16/16/4` pair 联合探测分别在 b-174-382、b-101-424 被独立控制室 candidate/ETA 约束拒绝，未得到 final report，不算预占负例。pair-only 短员产物只证明缺第三席边界，不证明完整 Box 普通补位融合。高持有 named `3` batch 为 9/10；唯一 b-112-404 是普通入口 warning-only 对应的既有周期心情 gate 负例，不是求解无解。以上失败与未验证项均保留在 evidence，不重分类为通过。
- **最终复核与范围**：按配置启动 Sol reviewer `gpt-5.6-sol` 返回 `Unknown model gpt-5.6-sol`；之后按用户指定的 `gpt-6.1-sol` 对冻结 diff 与 evidence 复核，结论为 `CONTINUE`，没有指出已证实回归，但要求补齐 pair-preclaim 局部失败的完整 Box 普通输出与同输入 gate 负例。因此 assertion diff 尚未获得 Sol `PASS`，ADR 实现状态保持 `in-progress`。本阶段验收未运行源码测试；源码测试不替代真实 Box 与 invariant gate。不扩大四班路径，也不关闭 `docs/TODO/排班统一内核重构方案.md` §7。

## 后果

正面后果：新的二/三班时长不再依赖 profile 枚举扩张；泡泡/火神只需一个 schedule-level 预占事实；报告、周期结算和 MAA 使用同一最终班次序列；四班复杂策略不被迫迁移。

代价与风险：目前 profile 名称仍承载队伍拓扑、菲亚动作槽和 MAA 宿舍提示等兼容含义，必须继续分离“时间输入”与“产品策略”；24h ETA 已统一到实际时长，但候选搜索仍是既定有界策略。现有 dirty ABC 预占会触及 generic layout 参数，迁移时须避免把干员专属逻辑下沉到通用 assignment 层。
