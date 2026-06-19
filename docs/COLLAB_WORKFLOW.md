# EffectAtom 协作工作流

与 [EFFECT_ATOM_DESIGN.md](EFFECT_ATOM_DESIGN.md) 第五节对齐。每轮会话建议 1–3 名干员。

## 节奏

1. **你点名**干员或设施类型（例：「贸易站下一批：诗怀雅、灵知」）
2. **Cursor 提案**：tier 划分、EffectAtom 表、**选层**（L1/L2/L3，见 [第八节](EFFECT_ATOM_DESIGN.md#八分层求解积木-vs-域短路定稿)）、与 `prts_trade_skills.json` 原文对照
3. **你确认**：对 / 错 / 一句补充（叠加规则、优先级等）
4. **Cursor 落地**：设计文档 → `skill_table.json`（含委托空 atoms）→ 必要时 `gold_flow` / `order_mechanic` / `trade_shortcuts.json` → 回归
5. **会话末**：新增词汇、下批推荐、第九节未决项

## 下批推荐（贸易站常数类）

> 贸易站 PRTS 干员 L1 已覆盖完毕 —— 以下干员已在 `skill_table` 注册，无新增词汇缺口：

| 干员 | 状态 | 说明 |
|------|------|------|
| 角峰 / 讯使 | ✅ | `AddFlatEff(15/20)` + `AddLimitDelta` |
| 银灰 | ✅ | `AddFlatEff(20)` + `AddLimitDelta(4)` |
| 诗怀雅 | ✅ | per-excess `AddFlatEff` |
| U-Official | ✅ | `eureka` tag + L2 |
| 雪雉 | ✅ | `TiandaoEffVarAllowed` + `PeerSettledEffSum` |
| 佩佩 | ✅ | `PeerEffAbsorb(0)` + `pepe_exclusive` |

## 数据文件职责

| 文件 | 维护者 | 说明 |
|------|--------|------|
| `EFFECT_ATOM_DESIGN.md` | 双方 | 词汇表 + 已建模干员 |
| `skill_table.json` | Cursor（你确认后） | buff_id → EffectAtom；空 atoms = 委托 L2（见第八节） |
| `trade_shortcuts.json` | 双方 | L3 组合表化最优解 + `verify` 锚点 |
| `operator_instances.json` | Cursor | **干员级真相**：`干员@tier_0` / `干员@tier_up` → `buff_ids` |
| `prts_trade_skills.json` / `.csv` | PRTS 快照 | **贸易站技能文字唯一可信来源**（[贸易站 §5 表格](https://prts.wiki/w/%E7%BD%97%E5%BE%B7%E5%B2%9B%E5%9F%BA%E5%BB%BA/%E8%B4%B8%E6%98%93%E7%AB%99) table[6]） |
| `prts_trade_skills_table.html` | PRTS 快照 | 上表原始 HTML |
| `MECHANICS_REGISTRY.csv` | 归档 | 不再用于贸易站技能核对 |

## 不变式（定稿）

1. `skill_table.id` 必须等于解包 `buff_id`（禁止 `skill_*`）
2. 干员归属只在 `operator_instances`（`resolve_buff_ids` 处理 `stepwise`）
3. 原文只信 `prts_trade_skills.json`
4. Pilot 干员（但书/可露希尔/孑/德克萨斯/拉普兰德/能天使）的 trade buff 必须在 `skill_table` 中存在

## 验证

```bash
python scripts/build_skill_table.py   # pilot 硬失败
cargo test -p infra-core              # 含 pilot_trade_buff_ids_resolve_in_skill_table
cargo run -p infra-cli -- verify --case reg_gsl_closure_tier90
```

回归失败时：先查 `trade_shortcuts.json` 是否过时，再查机制理解；**不**回退到运行时正则。
