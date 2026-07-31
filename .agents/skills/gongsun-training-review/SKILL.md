---
name: gongsun-training-review
description: 验收练卡推荐玩家知识（新手名单、组合、散件、输出口径）或对照旧 training_recommendations.json 时使用；先读 docs/练卡推荐相关，再逐条输出通过、修改或待裁决。
---

# 公孙长乐练卡推荐验收

玩家**产品语义**以 `docs/练卡推荐相关/` 为准，不接管机制公式、搜索剪枝或排班选型。

旧 `data/training_recommendations.json` 与已归档的 v2 机器诊断合同**不得**反向裁决玩家名单、组合名、≤10 人或五章报告结构。

## 必读来源（按任务选读，标 * 为默认）

1. * `docs/练卡推荐相关/README.md`（索引与真源顺序）
2. * 题面相关 canonical（可多份）：
   - `新手必练与必收集名单.md`
   - `玩家输出口径.md`（亦拥有领域键 `advice.training`）
   - `组合知识库.md`
   - `高效率散件与搓玉名单.md`
3. * `组合成员练度审阅稿.md`（已批练度/层级）
4. 实现计划（改代码时）：`玩家报告实现计划.md`
5. 代码 owner：`docs/PROJECT_MAP.md` 的 `training_advice` / `advice`
6. **仅当**对照旧运行时：归档 `docs/ARCHIVE/superseded/练卡推荐规则_v2机器诊断合同.md` + 可选 `python3 scripts/render_training_recommendations.py`

## 验收对象分流

| 对象 | 做法 |
|---|---|
| 玩家知识 / 口径 / 审阅稿 | 对照 canonical + 审阅稿；输出通过/修改/待裁决 |
| 旧 JSON 28 条 | 只作 legacy 对照；冲突以 `练卡推荐相关` 为准，建议记「实现重建」而非改 JSON 冒充新产品 |
| `advice --answer` 当前输出 | 实现未换 player 层前，允许判定为**相对产品合同不合格**；修代码走 feature 计划，不在本 skill 默认改 solver |

## 账号结果（实现重建前）

```bash
cargo run -p infra-cli -- advice --operbox <operbox.json> --answer
# 旧机器包
cargo run -p infra-cli -- advice --operbox <operbox.json> --pretty
```

夹具仍在 `data/fixtures/training_advice/`。验收玩家合同时以 `docs/练卡推荐相关/玩家输出口径.md` 的五章、≤10、C1–C10 等为准，**不要**要求输出必须匹配旧 `当前可练/获取后可练` 分区才算通过。

## 逐条门禁（玩家知识）

- 规范组合名、层级（核心/重要/次级/挂件）、目标练度以审阅稿 + 组合知识库为准。
- 新手层、特殊获取、C1 截断、C2 缺核心催收、搓玉门闩等以对应 canonical 为准。
- `standalone_roster.json` 不证明值得培养。
- `review`/冲突未决项标待裁决，不猜测。
- 不得为通过验收去改 solver 候选池。

## 旧规则表门禁（仅 legacy 对照时）

- v2 结构：`kind` / `scope` / `admission` / `members`；无用户 `message`。
- 与玩家合同冲突时结论写「应用 knowledge 重建替代」，禁止把旧 label（巫恋裁缝核等）写回产品文档。

## 输出格式

```text
[通过/修改/待裁决] rule_id 或 干员名或 合同条款
当前：…
依据：docs/练卡推荐相关/… 路径
意见：无，或一条可执行修改（文档或实现计划阶段）
```

汇总：通过/修改/待裁决数；本轮不处理的 solver/前端项。

## 修改边界

默认只读。落实裁决时：

- **产品语义** → 只改 `docs/练卡推荐相关/`（及用户授权的索引/路由）。
- **运行时玩家层** → 按 `玩家报告实现计划.md`，白名单 `training_advice` / knowledge；不改 layout/search。
- **旧 JSON** → 非默认；不得用其覆盖玩家 canonical。
