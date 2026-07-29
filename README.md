# ArknightsInfraCalc v2

一个面向《明日方舟》基建的效率求解、体系编排与轮换导出引擎。

它解决的不是“把三个最高百分比干员放进一间房”，而是：在玩家实际练度、房间容量、同一干员不能重复上岗、同房互斥、跨设施资源、体系硬核心和跨班绑定同时存在时，仍然给出可解释、可复现的排班。

项目最有价值的部分不是某张固定排班表，而是把基建专家知识拆成可以计算、搜索和回归验证的结构：

- 普通技能由声明式 `EffectAtom` 解释；
- 赤金链、订单分布和单位产出交给专门的领域引擎；
- 难以原子化的固定最优组合由 L3 shortcut 结算；
- 真正的硬约束由编排层保证，其余队友尽量由实际效率自然搜索；
- 贸易、制造、发电分别保留直接效率，不用一个匿名总分掩盖取舍。

## 三分钟理解它能做什么

给定一个练度盒（operbox）和一张基建蓝图，当前引擎可以：

| 能力 | 当前行为 |
|------|----------|
| 单房结算 | 计算贸易、制造、发电和中枢结果；贸易输出纸面效率、单位产出倍率、最终效率与规则 ID |
| 候选搜索 | 在合法池中枚举房间组合，并按各生产域的 `final_efficiency` 排序 |
| 单班编制 | 把 Rule/兼容 System 编译为 `AssignmentPlan`，再 Execute → Fill → Resolve，用全局 `used` 保证一人一岗 |
| 跨设施机制 | 汇总中枢注入、感知信息、人间烟火、木天蓼、虚拟发电等 producer，再让消费房读取 |
| 体系编排 | 表达 required anchor、同房 bond、禁止同房、配方限制、降级与班次绑定 |
| 定时轮换 | 默认生成 αβγ ABC `12h + 6h + 6h`；也支持二班 `12/12`、菲亚 `8/8/4/4` 和深海 `7/5/7/5` 具名 profile |
| 账号分析 | 对 operbox 生成练度画像，并与基准盒比较 |
| 导出 | 输出人类可读排班、账号画像 JSON 和 MAA 排班 JSON；core `BaseAssignment` 本身支持 serde |

贸易站是目前分层最完整的领域（L1 + L2 + L3）；制造、发电、中枢、全局资源和编排已经进入同一条全基建主路径，但各自的机制深度并不相同。当前事实地图见 [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)。

当前数据包含 727 条机制登记、298 个技能定义和 487 个练度实例（282 个逻辑干员）；编排同时使用 6 条声明式 Rule、8 个兼容 registry System 和 4 个中枢制造注入器，另有 16 个贸易 shortcut。仅控制中枢、贸易站、制造站的设施全集就分别对应 `C(55,5)=3,478,761`、`C(77,3)=73,150`、`C(90,3)=117,480` 个原始满房组合。这些数字不是“覆盖率 100%”的营销口号，而是为什么项目必须同时建设机制分层、候选池、Bake 与严格回归的工程尺度。事实入口见 [项目地图](docs/PROJECT_MAP.md)、[性能工程](docs/PERFORMANCE_ENGINEERING.md) 和 [已建模干员](docs/MODELLED_OPERATORS.md)。

## 架构一览

```text
用户当前裁决 → 对应领域 canonical Markdown
                         ↓ 由代码与生成 help 实现和证明
runtime data + operbox + BaseBlueprint
                         ↓
                    compute_plan
                         ↓
               一次用户 rotation probe
                         ↓
              schedule_timed_rotation
                         ↓
              assign_shift*（按 profile）
              ├─ build_plan → execute_plan
              ├─ 建池与各设施 fill / search
              └─ 反复 resolve → BaseAssignment
                         ↓
        TeamRotationReport → profile / CLI 文本 / MAA / Worker 响应
```

这里有一条重要边界：shortcut 负责“这个实际组合怎样结算”，不能代替“谁必须进编”；`shift_bind` 负责已入选成员怎样同上同下，也不能代替 required anchor。正因为这些责任被分开，项目才能同时容纳硬体系和自然搜索。

更完整的运行说明见 [docs/OVERVIEW.md](docs/OVERVIEW.md)，术语见 [docs/GLOSSARY.md](docs/GLOSSARY.md)。

## 直接运行一套真实方案

日常运行只需要能构建本 workspace 的 Rust / Cargo 工具链；Python 仅用于 `scripts/` 下的数据维护。仓库自带各星级最高合法练度的 243 夹具。下面的命令只计算一次用户 rotation，默认使用 αβγ ABC，并从同一结果生成账号画像和 MAA JSON：

```bash
cargo run -q -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/readme-profile.json \
  --maa-out out/readme-maa.json
```

只探测单班自定义布局：

```bash
cargo run -q -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text
```

运行机制回归入口：

```bash
cargo run -q -p infra-cli -- verify --all
```

生成并验证本机 Bake 候选表：

```bash
cargo run -q -p infra-cli -- bake all
cargo run -q -p infra-cli -- bake validate
```

`plan`、`layout team-rotation`、`layout eval`、`trade yield` 等完整参数见 [docs/INFRA_CLI.md](docs/INFRA_CLI.md)。

## 正确性优先，也认真对待性能

组合搜索会很快膨胀，但本项目不会用不透明剪枝换取一个看似漂亮的答案。

当前性能策略包括：

- 贸易等适用领域使用工具人池和 role policy 缩小候选边界；普通排班制造仍搜索全部合法普通制造候选；
- 单房组合求值可用 Rayon 并行，跨房则按稳定生命周期提交并重新 `resolve`；
- Bake 可生成 schema v12 的 3/2/1 人单房候选索引；运行时只有在数据指纹、布局、练度和动态上下文都兼容时才读取；仓库内旧 catalog 失配时会安全回退；
- Bake 不兼容、缺失或过期时回退到实时 solver。缓存失效只应影响速度，不能改变语义；
- `Efficiency` 用千分整数保存和排序，避免浮点尾差改变候选次序。

当前单班编制仍是领域约束下的分阶段、逐房搜索，不宣称全基建整数规划意义上的全局最优。动态贸易 producer 的统一联合搜索与完整候选列 Bake（A+）是**未来计划**，尚不是当前能力；设计边界见 [docs/TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](docs/TODO/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

## 为什么结果值得审计

项目把“可信”做成了可检查的工程链路：

1. 当前用户明确裁决优先；稳定业务语义由对应领域的唯一 canonical Markdown 拥有。
2. 代码和生成 help 证明当前实现事实；JSON、CSV、fixture 和 Bake 只承载或核对实现。
3. `skill_table.id == buff_id`，干员与技能归属只在 `operator_instances.json` 维护。
4. L1 解释器只认 `buff_id`，不会按干员中文名偷偷改公式。
5. 生产域统一输出三位小数直接效率；机制等效效率只作解释，不会重复乘入产出。
6. CSV 回归锚点、单位产出锚点、Rust 测试和真实 CLI 夹具覆盖不同责任层。
7. Debug 流程要求先复现、定位生命周期边界、删除冲突旧路径、补不变量回归，并保留可点击验证日志和 JSON 产物。

这不意味着仓库永远没有 bug；它意味着一个结果可以追到文档口径、数据条目、解释阶段、搜索边界、回归断言和实际 CLI 输出，而不是只能相信一张最终截图。

## 从哪里开始读

| 你是谁 / 想做什么 | 入口 |
|-------------------|------|
| 第一次了解项目 | [docs/OVERVIEW.md](docs/OVERVIEW.md) → [docs/GLOSSARY.md](docs/GLOSSARY.md) |
| 想沿一次真实请求看代码 | [docs/ARCHITECTURE_TOUR.md](docs/ARCHITECTURE_TOUR.md)、[docs/EXAMPLES/243_FULL_E2.md](docs/EXAMPLES/243_FULL_E2.md) |
| 懂基建策略，不关心代码 | [docs/GONGSUN_RUNTIME_OVERVIEW.md](docs/GONGSUN_RUNTIME_OVERVIEW.md) |
| 想运行 CLI 或接前端 | [docs/INFRA_CLI.md](docs/INFRA_CLI.md)、[docs/FRONTEND_CLI.md](docs/FRONTEND_CLI.md) |
| 想判断结果或性能是否可信 | [docs/QUALITY_AND_AUDIT.md](docs/QUALITY_AND_AUDIT.md)、[docs/PERFORMANCE_ENGINEERING.md](docs/PERFORMANCE_ENGINEERING.md) |
| 想理解机制建模 | [docs/EFFECT_ATOM_DESIGN.md](docs/EFFECT_ATOM_DESIGN.md)、[docs/EFFICIENCY_MODEL.md](docs/EFFICIENCY_MODEL.md) |
| 想理解编排与轮换 | [docs/BASE_ASSIGNMENT.md](docs/BASE_ASSIGNMENT.md)、[docs/排班模式.md](docs/排班模式.md)、[docs/SCHEDULE_ROTATION.md](docs/SCHEDULE_ROTATION.md) |
| 想修 bug / 审计体系 | [AGENTS.md](AGENTS.md)、[docs/MAINTENANCE_MODE.md](docs/MAINTENANCE_MODE.md)、[docs/SYSTEM_AUDIT_WORKFLOW.md](docs/SYSTEM_AUDIT_WORKFLOW.md) |
| 想查文件和模块 | [docs/INDEX.md](docs/INDEX.md)、[docs/PROJECT_MAP.md](docs/PROJECT_MAP.md) |

## 明确的非目标

- 宿管自动分配与完整心情恢复规划；
- 根据 ETA 自动改写所有班次时长；
- 全基建连续时间最优化；
- 对所有房间、所有班次做无界全局组合搜索；
- 用 CLI、fixture 或下游特判承载游戏机制；
- 把贸易、制造和发电揉成不可解释的匿名总分。

## 仓库结构

```text
crates/infra-core/   机制、搜索、编排、排班、导出
crates/infra-cli/    参数、加载、输出、回归命令
data/                运行时模型、体系、shortcut、夹具和锚点
docs/                当前事实、领域语义、维护流程、TODO 与归档
scripts/             数据构建与审计脚本
```

## License

MIT
