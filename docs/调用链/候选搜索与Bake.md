# 候选搜索与 Bake 调用链

> 文档角色：call-chain
> 生命周期状态：current
> 当前真源：docs/模块/候选搜索与Bake.md
> 摘要：从输入池、Plan 门禁和域搜索追踪到 comparator、Bake 命中或 live fallback
> 代码范围：crates/infra-core/src/{pool,search,bake.rs,candidate.rs}; crates/infra-cli/src/commands/{bake,profile}.rs
> 机制依赖：docs/知识库引用.md；vault:docs/1-基础设定/设施与进驻/基建总览.md；vault:docs/3-组合库/组合注册表.md
> 代码锚点：crates/infra-core/src/pool/base.rs::{build_roster_pool,filter_pool}; crates/infra-core/src/search/trade.rs::search_trade_triples; crates/infra-core/src/search/manufacture.rs::search_manufacture_triples; crates/infra-core/src/bake.rs::{bake_catalogs,validate_baked_catalog,try_baked_trade_search}
> 关联链路：docs/模块/候选搜索与Bake.md; docs/模块/中枢与跨设施状态.md; docs/调用链/跨设施状态刷新.md
> 核验基线：bake validate/verify、domain search report、plan --assert-invariants

## 触发与入口

`layout`、`plan` 和域内填房需要一个设施候选集合。CLI 可以通过 `bake` 预生成 catalog，也可以直接走 live search；两条路径都必须从同一 OperBox、runtime data、blueprint 和 Plan facts 构造输入。

```text
OperBox + instances + skill table + blueprint
  -> build_*_pool / filter_pool
  -> Plan claims / exclusion / used / capacity
  -> domain search (trade/manufacture/power/control)
  -> comparator or named policy
  -> SearchHit / report
  -> assignment commit
  -> resolve_base
  -> next room or next shift
```

## 候选门禁

pool builder 负责设施、tier、skill binding、人数和输入匹配；Plan 负责 required anchor、claim、exclusion、used 和跨设施 dependency。任一 hard constraint 失败都应跳过候选并保留可诊断原因，不能让排序分数把非法候选带回结果。

贸易搜索另有全布局产品禁配：黑键与巫恋不得同站。`search_trade_single_order` 在每种订单的候选过滤器中加入该对，live 搜索在排序前拒绝，role pick 复用同一入口；该全局禁配本身不强制退回全池，其他筛选条件仍按原逻辑决定候选池。Plan 对已同站的 seed / scratch 直接报错，最终 `--assert-invariants` 检查每班真实贸易房。

`top_k` 只限制报告行。完整池、standalone、shadow branch、producer prefix 和 `full_pool` 的保证等级必须在对应模块页声明；未证明等价的 reduction 只能叫 policy、heuristic 或 approximation。

## 搜索与提交

`search_trade_triples`、`search_manufacture_triples`、`search_power_assignment` 和 `search_control_combos` 只比较已经通过 admission 的组合。域 solver 结算实际组合并返回 breakdown；`layout/assign` 将命中提交到 assignment 后才允许下一阶段重新 resolve。候选搜索不负责 export 或 MAA 文本。

跨设施 producer 候选必须在当前 assignment 中同时满足 source、admission、target 和 relation；只存在 tag、能力或拥有名单不能提交 dependency。中枢 policy 读取冻结 ledger，不以跨域匿名总分替代 hard anchor。

## Bake 分支

`bake_catalogs` 用 live solver 生成带 schema、输入 fingerprint 和 combo-table hash 的 catalog。`validate_baked_catalog` 先检查结构、hash 和 row；`BakeMode::Auto` 对查询不兼容回 live，首次加载缺表或失效时先生成并校验，成功后查表，失败才回 live。`Disabled` 直接 live，`Required` 只读有效表并对 miss/invalid 返回错误。Bake 是候选缓存，不是机制或排序真源。制造带班长且池含暖机 atom 时整体回 live，按整班逐时平均结算；无班长仍保留 20 小时。

完整贸易行集合经 catalog 校验后，在查询行时按同一黑键—巫恋禁配过滤，再取前 K 条；该全局产品禁配可在 `Required` 下命中。其他未表示的禁配仍按原有 miss 语义处理；可执行文件或输入指纹不匹配的旧表不能跳过验证。

贸易行还保存单位特殊赤金产出能力；Bake 命中时使用与 live solver 相同的单位换算和总效率还原 `gold_consumption_per_day`，再交给 assignment commit。该隐藏字段不决定候选排序，普通订单的特殊产金值保持为零。

## 失败传播与排查

| 阶段 | 可见症状 | 首先检查 |
|---|---|---|
| 输入/池 | 干员未进候选、技能缺失 | OperBox、PromotionTier、instances、SkillTable 和 pool skip |
| Plan | 体系未激活、干员被抢 | required anchor、claim/exclusion、used、capacity |
| 搜索 | 组合未报告或结果顺序异常 | domain filter、comparator、具名 policy、top_k |
| Bake | catalog miss、hash 不匹配 | schema、input fingerprint、combo-table hash、mode |
| 提交/刷新 | 下一房间看到旧状态 | assignment commit 后的 `resolve_base`、global manifest、room snapshot |

排查先用 `layout eval --explain-json` 确认固定编制能否结算，再用 `bake validate/verify` 对账缓存，最后用 `plan --assert-invariants` 验证完整轮换。一次候选命中不能证明全局最优或知识库机制覆盖。
