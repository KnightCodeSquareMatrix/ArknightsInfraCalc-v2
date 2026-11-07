# TODO: 接入公孙长乐菲亚梅塔换心情优先级清单

> 文档角色：archive
> 生命周期状态：completed
> 替代项：docs/排班/Fiammetta.md；docs/排班/SCHEDULE_ROTATION.md
> 历史原因：规则和实现状态已吸收到 current owner
> 快照日期：2026-07-18
> 摘要：保存菲亚梅塔班次优先级实施完成记录

> 历史原状态：completed（2026-07-11）
> 来源：2026-07-10 用户需求
> 优先级来源：`C:\Users\KnightCode\Downloads\Fiammetta.md`

## 背景

菲亚梅塔换班模块已实现（`crates/infra-core/src/export/maa.rs`）。

核心逻辑：`resolve_fiammetta(priority, assignment)`——遍历优先级清单，找第一个在当班 assignment 里的干员作为换班目标；找到则输出 `enable: true`，找不到或清单为空则 `enable: false`（退化为旧行为）。

菲亚梅塔常驻宿舍，不占工作岗位，MAA 侧自动处理放技能后回宿舍的流程，无需代码干预。

## 待完成

- [x] **公孙长乐已提供优先级清单**（干员名列表，从高优先级到低优先级排列）
- [x] 将清单填入对应方案的 `MaaExportOptions.fiammetta_priority`
- [x] 补充对应的回归测试或 fixture

## 接入示例

```rust
opts.enable_gongsun_fiammetta_priority();
```

当前常规线性顺序为：`但书 > 巫恋 > 龙舌兰 > 清流 > 可露希尔`。布局动态排序、
龙巫跨班成组服务与完整心情求解属于后续排班策略层，不在本 TODO 的 MAA 单目标接线范围内。

## 后续接入进展

2026-07-11 已继续接入 ABC 排班主路径：每个 24 小时周期选择一次 peak 主力，
在其休息班放回原房间，换下当前在岗干员并重新评分；MAA 只在该班启用菲亚，
被换下者优先进入宿舍。动态布局规则与跨周期心情就绪仍未实现。

## 相关文件

| 文件 | 说明 |
|------|------|
| `crates/infra-core/src/export/maa.rs` | `resolve_fiammetta` / `MaaExportOptions.fiammetta_priority` |
| `crates/infra-cli/src/commands/plan.rs` | CLI 主入口，设置 `maa_opts` 的地方 |
| `crates/infra-cli/src/commands/layout.rs` | layout 子命令，同上 |
| `crates/infra-cli/src/commands/serve.rs` | 前端常驻 worker 的 plan 导出入口 |
| `docs/排班/Fiammetta.md` | 当前行为、已确认规则与后续策略边界 |
