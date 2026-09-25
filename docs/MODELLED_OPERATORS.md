# 运行时已建模干员与能力覆盖

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：`data/operator_instances.json`、`data/skill_table.json`、各域 interpreter；游戏机制见 [`知识库引用.md`](知识库引用.md)
> 代码范围：`crates/infra-core/src/{instances.rs,skill_table.rs,tier.rs,pool,trade,manufacture,control,cross_facility}`; `data/{operator_instances,skill_table}.json`
> 代码锚点：`crates/infra-core/src/instances.rs::{OperatorInstances::get,resolve_trade_buff_ids,resolve_manufacture_buff_ids}`; `crates/infra-core/src/skill_table.rs::{SkillTable::load,validate_operator_refs}`; `crates/infra-core/src/pool/trade.rs::build_trade_pool`; `crates/infra-core/src/trade/shortcut.rs::resolve_trade_shortcut`; `crates/infra-core/src/cross_facility/collector.rs::collect_global_atoms`
> 关联链路：`docs/模块/输入与技能映射.md`; `docs/调用链/技能到房间结算.md`; `docs/需要完成的干员建模.md`
> 核验基线：`947f1a2`

## “已建模”的可核验定义

1. `operator_instances.json` 有对应设施和 tier binding；
2. `skill_table.json` 有可解析 `buff_id`；
3. atom 已由对应域 interpreter 执行，或明确交给该域 L2/L3（例如贸易订单机制/shortcut）；
4. 跨设施能力在 `scope=global`、global pool/manifest 或 resolved layout 中有闭环；
5. 关键行为有 CLI/fixture/anchor 可复核。

空 atom 可能是 L2/L3 委托，也可能是缺口；不能仅凭表格行数宣称完整。非运行时设施、连续心情排班和未声明的游戏概率机制不在本表隐式扩展。

## 当前覆盖族

- 贸易：通用 Phase、订单分布、gold flow、特殊订单和 shortcut；组合规则见 [`INTERNAL/SHORTCUT_MATCHING.md`](INTERNAL/SHORTCUT_MATCHING.md)。
- 制造：配方效率、仓库、心情、部分 tagged/global inject；细节见 [`MANUFACTURE_STATUS.md`](MANUFACTURE_STATUS.md)。
- 中枢：结构化效果 ledger、global resource/inject、control trace。
- 发电/辅助：power、office、meeting 的有界静态求值；宿舍 global state 只在 collector 声明为 global 时进入链。

具体 operator 名单必须从当前 JSON 与 `SkillTable::validate_operator_refs` 生成，手工表格只作为导航。新增或修正干员时同时更新 instance、skill table、相关 L2/L3 数据、回归锚点和本页覆盖说明。
