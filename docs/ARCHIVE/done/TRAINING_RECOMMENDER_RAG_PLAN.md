# 练卡推荐规则与 RAG 实施计划

> 文档角色：archive
> 生命周期状态：completed
> 替代项：docs/产品与训练/README.md；docs/ARCHIVE/superseded/练卡推荐规则_v2机器诊断合同.md
> 历史原因：v2 规则、确定性过滤与伪 RAG 输入已落地，剩余外部规则裁决已拆分
> 快照日期：2026-07-20
> 摘要：保存练卡推荐 v2 与伪 RAG 输入协议的完成记录

## 0. 目标

给定一个 `operbox`，输出结构化练卡建议：

```text
now          当前可练
conditional  获取后可练
blocked      因核心未齐暂缓
ready        已达标
review       待人工复核
```

再把这份结果与体系分析文档原文交给 RAG，组织用户可读回答。

成功标准：

1. 规则表只保存结构化规则，不承担最终用户文案。
2. 程序只根据 `operbox` 与规则做确定性判定。
3. RAG 只能解释过滤结果，不能新增干员、改目标或改优先级。
4. 四类规则、核心准入、低星获取例外和多规则合并语义与 canonical 一致。

非目标：

- 设施数量 / 布局 / 生产目标条件过滤
- 战斗培养优先级
- 前端页面
- 完整 embedding 向量库
- 反向修改 solver 候选集合

## 1. 已冻结裁决

| 项 | 裁决 |
|---|---|
| 规则类型 | `system` / `combo` / `standalone` / `soft_combo` |
| 作用范围 | `same_station` / `cross_station` / `control_center` / `independent` |
| 输入 | 只看 `operbox` 现有字段 |
| 目标练度 | 规则表显式保存 `elite` / `level`；`skill_id` 只做来源核对 |
| 硬核心准入 | 必需核心已拥有才准入；未达目标只是训练缺口 |
| 核心未齐 | 该规则不产生 `train`；挂件也暂缓 |
| 核心已齐 | 推荐核心与已有挂件 |
| 低门槛获取 | 2/3/4 星默认可建议获取；指定赠送五星白名单（含苍苔） |
| 多规则命中 | 一条干员记录 + 全部 `matches` + 最高 `display_priority` |
| RAG 时机 | 过滤器先产出清单，RAG 再回答 |
| 待复核 | 进入 `review`，不伪装成确定推荐 |
| 文案 | 体系文档负责社区表达；规则表不写最终 `message` |

## 2. 架构

```text
体系分析文档 + 技能资料
        ↓
人工维护 training_recommendations.json (v2)
        ↓
规则校验
        ↓
operbox + rules
        ↓
确定性过滤器
        ↓
账号专属推荐包
        ↓
按 evidence 检索体系原文
        ↓
RAG 生成回答
```

## 3. 分阶段实施

### Phase A — 规格冻结

状态：完成

交付：

- [x] 更新 `docs/练卡推荐规则.md` 为当前合同
- [x] 将本 active change 改为实施计划
- [x] 旧 `message` 字段在 v2 中删除，不做兼容层

验收：

- 每个术语只有一个定义
- “核心不齐是否练已有核心”明确为否
- `acquire_then_train` 是正式动作类型

### Phase B — 规则 schema v2 与加载器

状态：完成

交付：

- [x] 新 `TrainingRecommendationRules` 类型
- [x] 加载 / 校验 `version: 2`
- [x] 全局 `acquisition_policy`
- [x] 四类 `kind` 与 `admission` / `members` / `evidence` / `review`
- [x] 拒绝非法组合：standalone 硬核心、hanger 进 required core 等

### Phase C — 确定性过滤器

状态：完成（含测试矩阵代表场景）

过滤器输出 `schema_version: 2`：

```json
{
  "schema_version": 2,
  "operbox_label": "...",
  "now": [],
  "conditional": [],
  "blocked": [],
  "ready": [],
  "review": []
}
```

干员记录含：`operator` / `action` / `display_priority` / `current` / `target` / `matches[]` / `source_refs[]` / `needs_review`。

### Phase D — CLI 输出切换

状态：完成

- [x] advice 命令直接序列化 `TrainingAdviceReport`（字段已切 v2）
- [x] 用 `data/operbox_gongsun.json` 完成 pretty + explain 真实账号核对

### Phase E — 全量规则迁移

状态：完成；外部来源裁决已拆分

- [x] `scripts/migrate_training_recommendations_v2.py` 将 v1 机械迁到 v2
- [x] `data/training_recommendations.json` 现为 version 2（28 条）
- [x] 完成 28 条中文验收稿核对并修正仓库 canonical 可证明项
- [x] 将 3 条外部 vault 才能裁决的 needs_review 规则拆到独立任务

### Phase F — RAG 输入协议与伪 RAG

状态：完成

- [x] 由过滤结果生成 `source_refs`
- [x] 按 path/heading/operator 检索仓库内 Markdown 片段
- [x] `--explain` 输出固定事实骨架、来源片段、不可用来源和 guardrails
- [x] 硬约束：不新增候选、不改 priority/target/action；review 不伪装成确定事实
- [x] 自定义规则来源限制在仓库根目录，绝对路径和目录逃逸不可读取

### Phase G — 收尾

状态：完成

- [x] 删除旧 schema 与冲突路径
- [x] 更新 gongsun 验收 skill / render 脚本
- [x] 用 evidence 工具记录 test / CLI / format / full-suite failure comparison
- [x] 吸收确认事实到 canonical，拆剩余开放项，归档本计划

## 4. 规则表 v2 最小样例

```json
{
  "version": 2,
  "acquisition_policy": {
    "default_rarity_le": 4,
    "named_exceptions": ["苍苔"]
  },
  "rules": [
    {
      "id": "standalone_clear_stream",
      "kind": "standalone",
      "scope": "independent",
      "label": "清流赤金散件",
      "admission": {
        "required_core": [],
        "pick_one_core": []
      },
      "members": [
        {
          "operator": "清流",
          "role": "independent",
          "target": { "elite": 1, "level": null, "skill_name": "水清则无鱼" },
          "priority": "P0",
          "acquisition": "suggest_acquire",
          "benefit": {
            "facility": "manufacture",
            "product": "gold",
            "note": "收益随贸易站数量变化，但不依赖硬队友"
          }
        }
      ],
      "evidence": [
        { "path": "docs/练卡推荐规则.md", "heading": "人工规则表" }
      ],
      "review": { "status": "confirmed", "conflicts": [] }
    }
  ]
}
```

## 5. 回归矩阵

| 场景 | 期望 |
|---|---|
| 核心齐、核心未达标、挂件已拥有 | 核心与挂件进入 `now.train` |
| 只有挂件、核心不齐 | 挂件不进 `now.train` |
| 部分核心拥有、部分缺失且不可获取 | 规则 `blocked`，无 `train` |
| 缺失核心可获取 | `conditional.acquire_then_train` + 后续计划；无 `now.train` |
| 独立散件已拥有未达标 | `now.train` |
| 独立散件未拥有但 2-4 星 | `conditional.acquire_then_train` |
| soft_combo 缺队友 | 已有成员仍可独立推荐 |
| 同一干员命中体系+散件 | 一条记录，全部 matches 保留，display_priority 取最高 |
| needs_review 规则 | 进入 `review`，不伪装确定推荐 |
| RAG 输入 | 不得包含候选包外干员 |

## 6. 当前进度

- 2026-07-19：用户确认产品逻辑与四类规则、核心准入、低星获取例外、RAG 边界。
- 2026-07-19：canonical `docs/练卡推荐规则.md` 按新合同重写。
- 2026-07-19：实现 v2 schema、加载校验、确定性过滤器、机械规则迁移、render 脚本；`cargo test -p infra-core training_advice` 14 通过。
- 2026-07-19：5 个 fixture CLI 验收：核心主语义通过；`conditional`/`blocked` 噪声与 ready/review 交叉待裁。
- 2026-07-19：文档路由补齐——`docs/INDEX.md` / `PROJECT_MAP.md` / `AGENTS.md` / canonical Agent 入口 / gongsun skill。
- 2026-07-20：收窄无关 blocked、完成跨分区单记录合并与命中来源收集；新增 N-of-M 核心组并修正红松林准入。
- 2026-07-20：新增 `advice --explain` 事实骨架与仓库内 Markdown 检索；路径边界和 review 隔离通过回归。
- 2026-07-20：28 条规则中 25 条 confirmed；3 条外部来源待裁决项拆至 `docs/TODO/练卡推荐规则表剩余人工验收.md`。
- 2026-07-20：targeted、renderer、workspace build、五个 fixture 和真实 operbox 通过；full-suite 相对 HEAD 无新增失败。

## 7. 历史说明

早期章节中的旧 schema 示意、`message` 字段和“未拥有一律不生成练卡任务”口径已被 canonical 取代。实现时只以 `docs/练卡推荐规则.md` 与本计划第 1–5 节为准。
