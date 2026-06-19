# ArknightsInfraCalc v2

明日方舟基建效率求解器（v2 绿场重写）。给定干员练度与布局假设，计算贸易站同房三人组的订单效率、机制等效效率、单位产出；支持穷举搜索、自定义蓝图探测与 **αβγ ABC 三队轮换**排班（`layout team-rotation`）。旧版 A-B-A 已废弃，见 [docs/SCHEDULE_ROTATION.md](docs/SCHEDULE_ROTATION.md)。

**当前主力域**：贸易站（L1 解释器 + L2 域引擎 + L3 组合短路 + 回归齐全）。制造站、控制中枢、全局资源已有基础实现，详见 [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)。

## 能做什么

| 能力 | 说明 |
|------|------|
| 单站求解 | 三人同房 → trade% / gold% / 机制等效 / 日产量 |
| 池搜索 | 从 operbox 可建模干员中穷举 C(n,3) Top-K |
| 自定义布局 | 加载 `BaseBlueprint` JSON，按你的基建结构搜索 |
| 三班轮换 | αβγ ABC 三队轮换（`layout team-rotation`） |
| 回归验证 | CSV 锚点 + 硬编码夹具，防止机制回退 |

**不做**：心情排班、宿管恢复、全基建连班优化——本求解器只输出效率，上层规划器再排班。

## 环境要求

- **Rust** 1.70+（workspace：`infra-core` + `infra-cli`）
- **Python** 3.14+（可选，仅 `scripts/` 数据维护；推荐 [uv](https://github.com/astral-sh/uv)）

```bash
# 克隆后
cargo build -p infra-cli
cargo test -p infra-core
```

## 快速开始

### 回归（无需 operbox）

```bash
cargo run -p infra-cli -- verify --all
cargo run -p infra-cli -- verify --case reg_gsl_closure_tier90
```

### 243 标准样例（layout + 全精2 operbox）

仓库自带 [data/fixtures/243/](data/fixtures/243/) 测试夹具，无需自备练度表：

```bash
# 243 布局 + 全精2 练度盒 → 贸易/制造 Top-K
cargo run -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text

# 同上 operbox，243 基准布局 bench
cargo run -p infra-cli -- bench \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text
```

### 自定义练度盒

也可使用自己的 operbox JSON（由 `scripts/xlsx_to_operbox.py` 从练度表导出）：

```bash
cargo run -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/operbox_gongsun.json \
  --text
```

### 三班模拟（默认：αβγ ABC 轮换 + MAA）

```bash
cargo run -p infra-cli -- layout team-rotation \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

### 其他常用命令

```bash
cargo run -p infra-cli -- pool --trade
cargo run -p infra-cli -- search trade --top 20
cargo run -p infra-cli -- trade yield closure_solo
```

完整子命令说明见 [docs/INFRA_CLI.md](docs/INFRA_CLI.md)。

## 仓库结构

```
crates/
  infra-core/     类型、EffectAtom 解释器、求解、搜索、排班
  infra-cli/      命令行：编排 + 输出 + 回归
data/             skill_table、干员实例、布局模板、回归用例（运行时真相源）
docs/             设计文档与模块地图
scripts/          Python：技能表构建、operbox 转换、数据审计
tests/fixtures/   最小 JSON 夹具
```

## 核心设计

求解流水线：

```
operbox + operator_instances + skill_table
    → pool → search / schedule
    → solve_trade_with_shift
        ├─ L1 interpreter      Phase 排序，只认 buff_id
        ├─ L2 gold_flow        赤金虚拟产线
        ├─ L2 order_mechanic   订单分布 → 机制等效
        └─ L3 shortcut         trade_shortcuts.json 表化最优解
    → unit_output
```

五条原则：

1. **游戏机制是唯一权威** — 从干员技能倒推 Selector / Action / Condition
2. **声明式 + 平坦** — 技能由 `data/skill_table.json` 的 EffectAtom 描述，运行时无文本解析
3. **L1 不认识干员名** — 解释器只按 Phase 执行 atom
4. **复杂机制分层** — L2 域引擎处理赤金链、订单分布；L3 短路处理固定最优组合
5. **效率求解，不排心情** — 给定场景假设 → 最高效率组合

机制细节：[docs/EFFECT_ATOM_DESIGN.md](docs/EFFECT_ATOM_DESIGN.md)

## 数据维护

| 文件 | 作用 |
|------|------|
| `data/skill_table.json` | buff_id → EffectAtom；空 `atoms` 表示委托 L2 |
| `data/operator_instances.json` | 干员 @tier → buff_ids |
| `data/trade_shortcuts.json` | L3 组合锚点 |
| `data/REGRESSION_CASES.csv` | verify 期望值 |
| `data/fixtures/243/` | **243 标准测试样例**（layout + 全精2 operbox + 排班导出） |
| `data/layout/*.json` | 基建蓝图模板 |

```bash
python scripts/build_skill_table.py   # 构建/校验 skill_table
python scripts/xlsx_to_operbox.py     # 练度表 xlsx → operbox JSON
```

个人练度表（`.xlsx`）与本地 operbox 导出默认已在 `.gitignore` 中，不会误提交。

## 开发与贡献

```bash
cargo test -p infra-core
cargo run -p infra-cli -- verify --all
```

- **Cursor / AI 协作**：先读 [AGENTS.md](AGENTS.md) → [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)
- **改 CLI**：见 [docs/INFRA_CLI.md](docs/INFRA_CLI.md)（禁止在 CLI 写求解公式）
- **待建模干员**：[docs/需要完成的干员建模.md](docs/需要完成的干员建模.md)

## License

MIT
