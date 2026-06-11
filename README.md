# ArknightsInfraCalc v2

明日方舟基建贸易站求解器 — EffectAtom 绿场重写。

## 原则

1. **游戏机制是唯一权威** — 从具体干员倒推 Selector / Action / Condition，见 [docs/EFFECT_ATOM_DESIGN.md](docs/EFFECT_ATOM_DESIGN.md)。
2. **声明式 + 平坦** — 技能由 `skill_table.json` 中的 EffectAtom 描述，运行时无正则、无 if/else 解析游戏文本。
3. **通用解释器** — L1 积木层不认识干员名，只按 Phase 执行 EffectAtom；链式/分布/表化组合见 [分层求解](docs/EFFECT_ATOM_DESIGN.md#八分层求解积木-vs-域短路定稿)。
4. **分层短路** — `gold_flow` / `order_mechanic` 为机制域最优解；`trade_shortcuts.json` 为组合表化最优解 + 回归锚点。

旧仓库（`ArknightsInfraCalc - 副本`）仅作归档参考，本仓库不迁移其 Rust 求解器代码。

## 结构

```
data/           机制注册表、干员实例、回归用例
docs/           EffectAtom 设计文档
crates/
  infra-core/   类型、解释器、贸易站求解
  infra-cli/    verify 命令
scripts/        数据校验脚本
tests/fixtures/ 最小排班夹具
```

## 使用

```bash
cargo test
cargo run -p infra-cli -- verify --case reg_gsl_closure_tier90
cargo run -p infra-cli -- verify --all
```

## 协作流程

每轮会话按 `EFFECT_ATOM_DESIGN.md` 第五节：你点名干员 → Cursor 提案 EffectAtom → 你确认 → 更新 `skill_table.json` 与回归。
