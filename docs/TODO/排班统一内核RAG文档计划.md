# 排班统一内核 RAG 文档计划

> 文档角色：active-change
> 生命周期状态：proposal
> 当前真源：docs/文档生命周期.md；docs/INDEX.md；docs/排班模式.md；docs/定时换班.md
> 摘要：为排班统一内核重构建立可分片检索、单向引用和事实状态可辨识的项目文档结构
> 代码范围：docs/**；crates/infra-core/src/schedule/**；crates/infra-core/src/export/**；crates/infra-cli/src/commands/**
> 代码锚点：docs/INDEX.md；docs/文档生命周期.md；crates/infra-core/src/schedule/team_rotation.rs::schedule_timed_rotation_request
> 关联链路：docs/调用链/定时轮换.md；docs/调用链/MAA导出.md
> 核验基线：HEAD 824ce88 + target/codex-runs/rotation-unification-20261018/status-before.txt

> 信息类型：plan
> 本页只规定文档拆分和 RAG 元数据，不把目标架构写成当前实现。文档治理修改必须和代码迁移的实际触点同步完成。

## 1. RAG 读取原则

检索一个问题时，默认按以下单向路径读取：

```text
症状 / 入口
  -> docs/INDEX.md / generated Agent route
  -> 调用链页
  -> 模块责任页
  -> canonical 领域页
  -> 知识库引用合同 / vault 原文
  -> 验证与 evidence
```

- `canonical` 裁决语义；`current-reference` 记录代码事实；两者不能互相伪装 owner。
- `active-change` 只回答计划、缺口和待迁移项；不能解释当前行为。
- `decision` 记录长期架构选择；必须同时标明实现状态。
- `target/codex-runs/`、`evidence/`、fixture、JSON 和 generated index 只提供证据或导航，不裁决产品规则。
- 每个 RAG chunk 必须保留文件路径、文档角色、生命周期状态、信息类型、核验基线和是否涉及 dirty overlay。

## 2. 每页固定元数据

所有新增 module/call-chain/interface 页使用现有治理字段，并补充以下正文标签：

```text
文档角色：canonical | current-reference | active-change | decision | archive
生命周期状态：current | proposal | in-progress | accepted | superseded
信息类型：current-fact | user-ruling | implementation-plan | checkpoint | evidence
当前真源：self（仅 canonical）或一个 canonical 文件
摘要：本页解决的唯一定位问题
代码范围：仓库相对路径或 glob
代码锚点：真实文件 + 稳定符号
关联链路：真实调用链页
机制依赖：知识库引用合同和章节
核验基线：commit/evidence + 命令 + 日期
```

正文每一条关键结论使用以下四种标签之一：

- `[事实]`：由当前代码、接口或已记录的用户裁决直接证明；
- `[用户裁决]`：写明日期和影响范围；
- `[推断]`：说明推断依据，不可作为产品语义；
- `[计划]`：未来结构，不可被当前调用链引用。

## 3. 目标文档分层

### 3.1 Canonical

保留一个领域键一个 owner：

- `docs/排班模式.md`：profile ID、wire 名、外部模式边界和不开放的入口；
- `docs/定时换班.md`：固定轮换的用户可见策略、心情预算、休班底线和错误级别；
- `docs/菲亚梅塔使用规则.md`：动作槽位、ready、目标去重、输出语义；
- `docs/接口/输入输出与兼容.md`：输入、Report、MAA/Worker 字段和兼容矩阵；
- 回收站机制语义在知识库桥接稳定前继续由 `docs/TODO/回收站选人放人与轮换设计.md` 作为 proposal，不把 dirty 代码提升为 canonical。

### 3.2 Current-reference

按问题切片，不重复完整规则表：

- `docs/模块/排班职责地图.md`：当前源码模块 owner、依赖和迁移状态；
- `docs/模块/排班与轮换.md`：排班模块接口、当前数据结构和失败边界；
- `docs/模块/心情与辅助设施.md`：心情/ETA/宿舍/辅助设施 runtime owner；
- `docs/模块/导出与岗位状态.md`：BaseAssignment 到 MAA 的单向投影；
- `docs/调用链/定时轮换.md`：当前入口到 Report 的状态流；
- `docs/调用链/MAA导出.md`：Report 到 MAA 的投影和不重算原则；
- `docs/接口/CLI与Worker.md`：命令、Worker、版本和请求响应边界。

这些页的“当前真源”只能指向 canonical，不能互相指为最终语义 owner。

### 3.3 Active-change

- `docs/TODO/排班统一内核重构方案.md`：代码迁移边界、阶段、行为等价和停止条件；
- `docs/TODO/排班统一内核RAG文档计划.md`：文档分层、检索和事实标签；
- `docs/TODO/排班行为等价验证矩阵.md`：old/new 差分和证据要求；
- `docs/TODO/回收站选人放人与轮换设计.md`：回收站未决产品策略和外部协议提案；
- 旧调查 checkpoint：只留在 `target/codex-runs/`，不得重新复制到 current 文档。

## 4. 当前发现的文档治理风险

- `docs/模块/排班与轮换.md` 与 `docs/SCHEDULE_ROTATION.md` 当前互相作为真源，形成环；迁移时必须选一个 canonical 根，另一页只做索引或归档。
- profile 闭集、时长和裸 `4` 限制在 `docs/排班模式.md`、`docs/SCHEDULE_ROTATION.md`、模块页、调用链页和接口页重复；最终只由 `docs/排班模式.md` 裁决，其他页链接。
- 周期心情预算在定时换班、模块、调用链、心情页和错误页重复；最终只由排班 canonical 裁决，模块页写 owner，调用链写状态流。
- 回收站初期调查曾存在未跟踪源码与文档状态不一致的问题；当前模块地图和协议页已部分同步，源码分批提交状态见统一内核方案的接手 checkpoint。提案未完成的部分仍须标记 partial，不因文件已经提交就宣称所有迁移目标已实现。
- `MAA` 导出使用 warning 文本前缀决定部分 autofill 行为；文案不是普通展示文本，文档和重构不得未经差分修改。

## 5. 文档完成验收

- 所有新增页有合法角色、状态、当前真源、摘要、代码范围、代码锚点、关联链路和核验基线；
- 只有 canonical 使用 `当前真源：self`；
- current-reference 只引用 canonical，不互相循环；
- 调用链页按入口、状态、分支、错误、输出、验证、缺口顺序组织；
- active-change 的计划、checkpoint、未决项不会出现在 current 文档的事实段；
- `generate_agent_index.py --check`、`docs_inventory.py --check` 和 `check_repository_facts.py` 的失败必须如实记录，不以删页或伪造索引消除；
- 文档更新只吸收已经验证的当前行为，未验证的设计保留在 TODO/ADR。
