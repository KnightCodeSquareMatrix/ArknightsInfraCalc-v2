# Scoring Phase 3：公式接口预留

> 文档角色：archive
> 生命周期状态：completed
> 替代项：docs/机制/EFFICIENCY_MODEL.md；docs/机制/SCORING_MODEL.md
> 历史原因：评分分量和 policy 已吸收到 current owner
> 快照日期：2026-07-18
> 摘要：保存评分 Phase 3 实施完成记录

> 历史原状态：done
> 来源：[SCORING_REFACTOR_PLAN.md](../plans/SCORING_REFACTOR_PLAN.md) Phase 3
> 完成日期：2026-06-24
> 约束：新增接口，不接真实公式，不改变当前排序行为。
> 当前说明：本归档记录的是旧 Phase 3。当日后续已按公孙长乐意见改为**分量化评分 policy**，不再等待贸易-制造平衡公式；当前实现见 `crates/infra-core/src/scoring/components.rs` 和 [SCORING_MODEL.md](../../机制/SCORING_MODEL.md)，历史计划见 [SCORING_REFACTOR_PLAN.md](../plans/SCORING_REFACTOR_PLAN.md)。

## 目标

新增 `infra-core::scoring` 模块，把跨贸易 / 制造 / 全局注入的复合效率换算收敛到唯一入口。公孙公式未到前只放 placeholder，不能发明权重。

## 改动范围

| 文件/目录 | 动作 |
|-----------|------|
| `crates/infra-core/src/scoring/mod.rs` | 新增模块门面 |
| `crates/infra-core/src/scoring/metric.rs` | 定义评分单位 / 输出类型 |
| `crates/infra-core/src/scoring/balance.rs` | 定义公式 ID、输入、placeholder 入口 |
| `crates/infra-core/src/lib.rs` | 暴露 `scoring` 模块 |
| `crates/infra-core/src/search/control.rs` | 在中枢裸加口径处接入 placeholder 调用点，不改排序结果 |

## 最小接口

```rust
pub enum BalanceFormulaId {
    Placeholder,
    GongsunTradeManuV1,
}

pub struct TradeManuBalanceInput {
    pub trade_eff_pct: f64,
    pub gold_manu_eff_pct: f64,
    pub battle_record_manu_eff_pct: f64,
    pub trade_station_count: u8,
    pub gold_line_count: u8,
    pub battle_record_line_count: u8,
}

pub struct BalancedEff {
    pub formula: BalanceFormulaId,
    pub composite_eff_pct: f64,
}
```

## 验收

- [x] `cargo test -p infra-core --no-run` 编译通过，日志落到 `target/codex-logs/`。
- [x] `cargo test -p infra-core --quiet` 通过。
- [x] placeholder 单测只锁定接口行为和公式 ID，不把 placeholder 数值当最终理论锚点。
- [x] 当前 trade/manufacture/power/control 排序结果不因本阶段改变。
- [x] `cargo build -p infra-cli` 通过。
- [x] `cargo run -q -p infra-cli -- verify --all` 通过。

## 后续

- 等公孙长乐贸易-制造平衡公式与锚点到位后，在 `crates/infra-core/src/scoring/balance.rs` 实现 `GongsunTradeManuV1`。
- 中枢评分已通过 `placeholder_trade_manu_balance` 进入公式入口；真实公式接入时优先替换该入口行为。
