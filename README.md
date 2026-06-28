# ArknightsInfraCalc v2

明日方舟基建效率 / 编排 / 排班研究型求解器。

这个项目把明日方舟基建视为一个带有大量领域结构的 NSP（Nurse Scheduling Problem）/ 资源编排问题：如果直接在全干员、全设施、全班次、全约束上搜索，它天然接近 NP-hard 的组合爆炸；但游戏机制本身并不是随机噪声，它包含大量可解释、可验证、可复用的专家结构。

ArknightsInfraCalc v2 的核心目标，是把这些专家结构编码进求解器：用机制分层、体系 anchor、固定最优组合、跨设施资源注入和命名 scoring policy，将原始的大规模搜索空间降维为若干可解释的局部搜索、约束满足与分量化排序问题。

换句话说，它不是一个只追求“给出最高分”的黑箱工具，而是一个研究如何把复杂游戏调度问题工程化、可解释化、可回归验证的范例。

## 问题口径

给定：

- 一个玩家的干员练度盒（operbox）；
- 一个基建布局蓝图（如 243）；
- 技能、体系、shortcut、全局资源等机制事实；
- 若干业务假设（贸易站订单、制造产线、控制中枢注入、轮换方案）。

求解器输出：

- 单站贸易 / 制造 / 发电 / 中枢效率；
- 贸易订单效率、赤金效率、制造经验效率等分量；
- 全基建单班进驻编制；
- αβγ ABC 三队轮换；
- 账号画像与 MAA 排班 JSON。

项目刻意不把所有目标揉成匿名综合分。跨贸易、制造、power、global inject 的结果优先按分量展示；确实需要排序时，必须进入命名 `scoring` policy。

## 方法论

本项目的基本研究判断是：

> 不要试图用一个通用公式吞掉整个基建。先承认问题的组合复杂性，再用专家策略把它压到可求解、可解释、可验证的子空间。

因此，代码结构按层承担不同责任：

| 层 | 作用 |
|----|------|
| 机制事实 | `skill_table.json` / `operator_instances.json` 记录 buff_id、干员实例与 EffectAtom |
| L1 解释器 | 只认 `buff_id`，按 Phase 执行 Selector / Condition / Action |
| L2 域引擎 | 处理赤金链、订单分布、单位产出等机制域最优解 |
| L3 shortcut | 表化固定最优 / 难 atom 化组合，作为专家策略锚点 |
| 体系编排 | 将 anchor、producer、constraint、degradation 转成 `AssignmentPlan` |
| 局部搜索 | 只在剩余自由度中穷举或贪心搜索 |
| scoring policy | 对需要排序的混合结果使用命名 policy，避免隐式混量纲 |

这套分层使得“专家策略”不是口头经验，而是可以被数据文件、代码模块、回归锚点和文档共同约束的工程对象。

## 当前能力

| 能力 | 说明 |
|------|------|
| 单站求解 | 三人同房 → trade% / gold% / 机制等效 / 日产量 |
| 池搜索 | 从 operbox 可建模干员中穷举 C(n,3) Top-K |
| 自定义布局 | 加载 `BaseBlueprint` JSON，按指定基建结构求解 |
| 体系编排 | 通过 `base_systems.json` 与代码化体系层认领固定组合、anchor 与降级路径 |
| 三班轮换 | αβγ ABC 三队轮换（`plan` 或 `layout team-rotation`） |
| 账号画像 | 练度分析 + 排班建议（`plan` / `layout analyze`） |
| 回归验证 | CSV 锚点 + 硬编码夹具，防止机制回退 |
| MAA 导出 | 输出可供 MAA 使用的排班 JSON |

当前主力域是贸易站：L1 解释器、L2 域引擎、L3 shortcut 与回归验证较完整。制造站、控制中枢、发电站、全局资源和编排层已有基础实现，并继续向分量化 scoring 与体系编排收敛。

非目标：心情排班、宿管恢复、全基建连续时间最优化。这些属于更上层规划器；本项目聚焦效率求解与可解释编排。

## 快速开始

### 环境要求

- Rust 1.70+（workspace：`infra-core` + `infra-cli`）
- Python 3.14+（可选，仅 `scripts/` 数据维护；推荐 [uv](https://github.com/astral-sh/uv)）

```bash
cargo build -p infra-cli
cargo test -p infra-core
```

### 回归验证

不需要 operbox，直接验证机制锚点：

```bash
cargo run -p infra-cli -- verify --all
cargo run -p infra-cli -- verify --case reg_gsl_closure_tier90
```

### 一体化方案

默认 243 布局；`--operbox` 支持 JSON 或一图流 xlsx。该入口会执行账号分析、αβγ 三队轮换，并可导出 MAA JSON。

```bash
cargo run -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

### 243 标准样例

仓库自带 [data/fixtures/243/](data/fixtures/243/) 测试夹具，无需自备练度表：

```bash
cargo run -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text
```

### 仅排班 + MAA

```bash
cargo run -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

### 其他常用命令

```bash
cargo run -p infra-cli -- bench \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text

cargo run -p infra-cli -- pool --trade
cargo run -p infra-cli -- search trade --top 20
cargo run -p infra-cli -- trade yield closure_solo
```

完整子命令说明见 [docs/INFRA_CLI.md](docs/INFRA_CLI.md)。

## 求解流水线

```text
operbox / roster + operator_instances + skill_table
        ↓
  pool（可建模干员池，C(n,3) 组合基数）
        ↓
  search（穷举三人组，rayon 并行） 或 schedule（三班贪心逐站）
        ↓
  solve_trade_with_shift（单站核心）
        ├─ L1 interpreter      Phase 排序 → Selector / Condition / Action
        ├─ L2 gold_flow        赤金虚拟线链
        ├─ L2 order_mechanic   订单分布 → 机制等效效率
        └─ L3 shortcut         trade_shortcuts.json 表化最优解
        ↓
  unit_output                  单位贸易量 / 无人机 / 日产量
```

关键约束：

1. `skill_table.id` 必须等于解包 `buff_id`。
2. 干员归属只在 `operator_instances.json`。
3. L1 解释器不认识干员名，只解释机制 atom。
4. `atoms: []` 可以表示委托给 L2 / L3 / 特例域处理，不等于未建模。
5. 贸易搜索 `score == trade_pct == order_eff_total`；`gold_pct` 单独展示。
6. 跨域结果需要排序时，只能通过命名 scoring policy。

机制细节见 [docs/EFFECT_ATOM_DESIGN.md](docs/EFFECT_ATOM_DESIGN.md)，评分口径见 [docs/SCORING_MODEL.md](docs/SCORING_MODEL.md)。

## 仓库结构

```text
crates/
  infra-core/     类型、EffectAtom 解释器、求解、搜索、编排、排班
  infra-cli/      命令行：plan / layout / verify / 输出
data/             skill_table、干员实例、体系、shortcut、布局模板、回归用例
docs/             文档入口、模块地图、TODO、ADR、归档、设计参考
release/          前端发布包（infra-cli/infra-cli.exe、layout-gen、fixtures）
scripts/          Python：技能表构建、operbox 转换、数据审计
tests/fixtures/   最小 JSON 夹具
```

文档入口见 [docs/INDEX.md](docs/INDEX.md)，项目地图见 [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)。AI / agent 协作请先读 [AGENTS.md](AGENTS.md)。

## 数据真源

| 文件 | 作用 |
|------|------|
| `data/skill_table.json` | `buff_id` → EffectAtom；空 `atoms` 表示委托域引擎 |
| `data/operator_instances.json` | 干员 @tier → buff_ids，是干员归属唯一真相 |
| `data/trade_shortcuts.json` | L3 组合锚点与固定最优策略 |
| `data/base_systems.json` | 编排层体系认领 |
| `data/REGRESSION_CASES.csv` | CLI verify 期望值 |
| `data/UNIT_OUTPUT_ANCHORS.csv` | 单位产出 / 赤金锚点 |
| `data/fixtures/243/` | 243 标准测试样例 |
| `data/layout/*.json` | 基建蓝图模板 |

```bash
python scripts/build_skill_table.py
python scripts/xlsx_to_operbox.py
```

个人练度表（`.xlsx`）与本地 operbox 导出默认已在 `.gitignore` 中，不会误提交。

## 开发入口

```bash
cargo test -p infra-core
cargo run -p infra-cli -- verify --all
```

- 评分 / 排序口径：[docs/SCORING_MODEL.md](docs/SCORING_MODEL.md)、[docs/SCORING_REFACTOR_PLAN.md](docs/SCORING_REFACTOR_PLAN.md)
- 编排 / 体系 / meta 组合：[docs/ADR/0001-layout-assignment-decomposition.md](docs/ADR/0001-layout-assignment-decomposition.md)
- CLI / 前端集成：[docs/INFRA_CLI.md](docs/INFRA_CLI.md)、[docs/FRONTEND_CLI.md](docs/FRONTEND_CLI.md)
- 准备实现事项：[docs/TODO/](docs/TODO/)
- 归档材料：[docs/ARCHIVE/](docs/ARCHIVE/)
- 待建模干员：[docs/需要完成的干员建模.md](docs/需要完成的干员建模.md)

## License

MIT
