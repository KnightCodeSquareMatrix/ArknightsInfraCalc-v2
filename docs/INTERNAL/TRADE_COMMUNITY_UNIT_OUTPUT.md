# 贸易单位产出锚

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/模块/贸易求解.md
> 代码范围：`crates/infra-core/src/trade/{shortcut,solver,unit_output}.rs`; `data/trade_shortcuts.json`; `data/REGRESSION_CASES.csv`
> 代码锚点：`crates/infra-core/src/trade/shortcut.rs::CommunityUnitOutputRule`; `crates/infra-core/src/trade/shortcut.rs::validate_trade_shortcuts`; `crates/infra-core/src/trade/solver.rs::resolve_unit_output`; `crates/infra-core/src/trade/unit_output.rs::{compute_unit_output,daily_yield}`
> 关联链路：`docs/模块/贸易求解.md`; `docs/INTERNAL/SHORTCUT_MATCHING.md`
> 核验基线：`947f1a2`；2026-10-29但书单点核对使用固定知识库`977b6ab`与当前回归输入
> 摘要：说明特殊贸易组合单位产出的数据锚、精度与验证入口

特殊组合的社区单位产出是 L3 产出锚，不由等效效率反推，除非条目标记 `derived_from_rounded_equivalent`。每个改变单位产出的 shortcut 必须同时声明 source 和 precision；非有限、非正数、缺来源或重复 id 在加载时失败。

运行时由 `resolve_unit_output` 选择 regular/L3 unit，再由 `TradeProductionReport::daily_at_shift` 计算工作时长产出。`mechanic_equivalent_efficiency` 不参与第二次乘法。修改锚点时同步更新 JSON、来源注释、回归期望和 verify fixture。

`reg_gsl_docus_solo`的精2三级单位产出锚为15929.2（固定知识库`977b6ab`的但书单站canonical）；1.55是短倍率展示，不能同时作为无限精度锚。该fixture纸面效率1.230，reference=10265，千分倍率`round(15929.2/10265,3)=1.552`，最终等效效率`round(1.230×1.552,3)=1.909`；实际24h龙门币按原锚结算为`15929.2×1.230=19592.916`，不由最终等效效率二次反推。

2026-10-29已将CSV过期期望1.907修为1.909。1.907对应旧锚15910.75（10265×1.55），当前锚更新后期望未同步；机制解释0.550、容差0.001、fixture、单位产出JSON及core公式均保持。该单点复核为固定来源和独立算术证据；最终CLI `7695096fd...`完整Bake/回归已exit0，`reg_gsl_docus_solo`为PASS（1.909、0.550）。不证明其他未覆盖组合或等级；详见`target/codex-runs/perf-final-20261028/correctness-docus/report.md`。

暖机源码修改会改变`None`的20h表数值与exe指纹；原binary/旧表/第一轮对账保留为旧语义历史。最终CLI `7695096fd...`已在`linear-warm-final/baked`隔离目录重新生成完整Bake并全量verify，29,783,417行/3,292兼容查询差异0、2,148查询门禁跳过，CLI exit0、但书PASS。CSV修正不改变锚/fixture/公式，不手改表或指纹；最终回归证明来自新binary，不以旧完整表或暖机局部验证替代。
