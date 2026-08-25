# AI 编排 Harness Phase 0A 协议草案

> 文档角色：active-change
> 生命周期状态：proposal
> 当前真源：docs/文档生命周期.md；docs/QUALITY_AND_AUDIT.md；docs/PROJECT_MAP.md
> 摘要：冻结 harness-neutral 的任务、事件、结果、证明义务、证据 overlay 和 runtime capability 合同

## 1. 目标

Phase 0A 只定义跨 MiMo、OpenCode、Claude Agent SDK 和 OpenAI runtime 可适配的协议，不选择长期框架，不修改 `infra-core`，不改变现有 `scripts/codex/run_evidence.sh` 的真源语义。

机器可读合同：

- [agent-harness-v0.schema.json](../../contracts/agent-harness-v0.schema.json)
- [agent-harness-capability-matrix-v0.json](../../contracts/agent-harness-capability-matrix-v0.json)

## 2. 不变量

1. 任务状态必须可恢复、可取消、可去重；重复请求不能隐式执行两次副作用。
2. `verified` 是 completion gate 的唯一通过状态；`pending`、`partial`、`failed`、`blocked`、`not_run` 一律不能通过。
3. Evidence overlay 只能引用现有 evidence manifest、status、log 和 artifact，不改写 evidence 真源。
4. 第一阶段只允许公开仓库材料和公开 fixture；私有 operbox、assignment、layout 和远程 provider 输入默认 blocked。
5. 只读 child 不得 edit、bash、task 或写入主工作树；当前实际 profile 映射不能凭空创建。
6. 领域语义仍以 canonical Markdown 和用户裁决为准；claim 必须区分 fact、inference、unknown、unsupported、policy 和 decision。
7. CPU-bound 项目的 benchmark 并发默认 1；资源策略在任务启动时确定，而不是运行中临时猜测。

## 3. TaskRecord

TaskRecord 至少保存：

- `id`、`protocol_version`、`project_profile`、`state`；
- `base_sha`、`dirty_worktree`、`input_sha256`；
- runtime、runtime version、project profile；
- `write_policy`、data classification、resource class；
- child tasks、依赖、session id、worktree；
- append-only child events；
- proof obligations、claims、artifacts；
- retry idempotency key、cancel/resume 结果。

## 4. ChildEvent

允许事件：

```text
queued → started → progress/waiting → succeeded
                           ↘ failed/cancelled/expired
```

恢复使用 `resumed` 事件。事件必须带 task id、发生时间和幂等 key。事件日志追加，不用后来的摘要覆盖原始事件。

## 5. ResultContract

每个只读 child 至少返回：

- findings；
- claims；
- 文件、类型、函数或行号来源；
- unknowns；
- negative evidence；
- guarantee class；
- canonical conflict；
- reviewer-required；
- artifact 引用。

自然语言自我总结不能单独成为 evidence。

## 6. Proof obligations 与 EvidenceOverlay

Proof obligation 是任务必须证明的事项，不等于命令列表。示例：

```text
no_double_count       invariant
real_layout_entry     cli
regression             targeted_test
```

EvidenceOverlay 记录 obligation 到现有 manifest/status/log/artifact 的引用和状态。它不复制、不重写、不替代 `target/codex-runs/` 内的原始证据。

Completion gate 使用 allow-list：所有 required obligation 必须明确为 `verified`，否则整体为 blocked。

## 7. CapabilityMatrix

runtime capability 只能标记：

```text
available
documented
documented_beta
model_directed
prompt_contract
workflow_defined
application_defined
adapter_required
unverified
unsupported
blocked
```

`unverified` 不能被当作 `available`。runtime 不支持某能力时，adapter 必须返回 `unsupported` 或 `blocked`。

当前项目实际 profile 只包括：

```text
plan-reviewer
terra-explorer
luna-extractor
sol-reviewer
```

## 8. Snapshot 与数据边界

Phase 0B/0C 使用公开材料，不使用真实私有编制。后续引入私有输入前，需要补：

- 私有 artifact 路径；
- stdout 脱敏规则；
- provider 允许范围；
- transcript/memory retention；
- telemetry policy；
- 输入和 artifact hash；
- worktree/base SHA 绑定。

## 9. Capability preflight 验收

每个候选 runtime adapter 至少演示：

1. 创建 TaskRecord；
2. 创建一个 child task；
3. 产生 queued/running/succeeded 或 failed 事件；
4. 返回符合 ResultContract 的结构化结果；
5. 生成 EvidenceOverlay 引用；
6. 对不支持的 resume/cancel/privacy/worktree 能力返回明确状态；
7. 不修改业务代码和主工作树。

## 10. Scope 与后续阶段

本草案不包括：

- 单 writer executor；
- worktree 自动 merge；
- benchmark runner；
- Dream/Distill；
- 自动 memory 晋升；
- 私有 operbox/assignment 远程执行；
- solver、candidate、schedule、cross-facility 业务变更。

通过协议审阅和公开材料 preflight 后，才进入 Phase 0C `research-wave`。

## 11. Phase 0B 当前结果

已完成静态 preflight：

- 工具：`scripts/codex/preflight_harness.py`；
- 测试：`scripts/codex/tests/test_preflight_harness.py`；
- 报告：`target/codex-runs/harness-phase0a/preflight/phase0b-rerun.json`；
- 当前实际 profile：`terra-explorer`、`luna-extractor`、`sol-reviewer`；
- 三个 profile 的 `edit`、`bash`、`task` 均为 `deny`；
- 外部 runtime 均保持 `unverified`；
- 未启动 runtime，未读取私有输入，未运行业务命令。

第一次运行暴露了 `plan-reviewer` 仅在全局 Task permission 中存在、但不在项目本地 profile 的配置漂移；matrix 已修正，测试重新通过。

这只完成静态边界检查，不代表 MiMo、Claude 或 OpenAI runtime 已兼容。下一阶段是公开材料 adapter preflight 和 `research-wave` 设计。

## 12.1 Native subagent research-wave join

已在 OpenCode root 会话中直接并行调用当前项目原生 subagent，未经过外部 CLI adapter、MCP 或 Worker Broker：

```text
layout-owner          terra-explorer  succeeded
cross-facility-owner  terra-explorer  succeeded
evidence-entry        luna-extractor  succeeded
```

Join artifact：`target/codex-runs/harness-phase0c/reports/native-subagent-wave-join.md`。

本次执行验证：

- root agent 可以调度项目 subagent；
- 三个 child 都返回了符合 ResultContract 形状的 findings、claims、sources、unknowns 和必要的 negative evidence；
- 只读 profile 的 edit/bash/task 禁止生效；
- public scope 和 child execution 均为 `verified`；
- cross-facility 领域冲突使 semantic join 和 overall gate 保持 `blocked`。

这完成了 Phase 0C 的原生 subagent 执行与 fail-closed join 验证。`blocked` 是领域语义审阅状态，不是 runtime 或 worker 失败。Durable resume/cancel、跨 provider worker 和私有输入仍属于后续阶段，不进入本轮扩张。

## 13. Phase 1 completion gate 当前结果

已实现最小 Evidence Control Plane 门禁：

- `scripts/codex/completion_gate.py`：只读取 `agent-harness-v0` envelope；`research-wave-v0` 只能作为 `task.research_wave` specialization；
- `scripts/codex/tests/test_completion_gate.py`：覆盖全部 required obligation 通过、非 `verified` 阻塞、非法输入拒绝和嵌套 TaskRecord；
- completion gate 只允许 required obligation 的状态全部为 `verified` 时返回 `verified`；`pending`、`partial`、`failed`、`blocked`、`not_run` 均返回 `blocked`。

已验证：

```text
standard_library_tests       = verified
pending_obligations          = blocked
all_required_verified        = verified
```

本阶段仍不修改 `run_evidence.sh` 的 manifest 真源，不运行私有业务任务，不进入 writer/worktree/resource runner。下一步只有在需要把 evidence manifest 自动映射到 proof obligation 时，才增加 EvidenceOverlay 生成器；当前 completion gate 已能对结构化 TaskRecord 执行 fail-closed 判断。

范围门禁补充：`render_evidence.py` 已成功渲染 Phase 1 manifest；`check_task_scope.py` 未通过，因为当前共享工作区包含其他任务的既有改动，且本轮 manifest 尚未登记最终 reviewer metadata。该失败不归因于 completion gate 代码，不能将其升级为本轮范围已审阅或整体任务完成。

## 13.1 EvidenceOverlay v0 当前结果

已实现 `scripts/codex/build_evidence_overlay.py`，将 TaskRecord/research-wave 的 `proof_obligations.evidence_refs` 显式映射到现有 evidence manifest 的 run id、artifact id 或 artifact path。

映射规则保持保守：

- 没有 `evidence_refs`：`not_run`；
- 引用不存在：`blocked`；
- 引用任一 FAIL run：`failed`；
- 所有引用均为 PASS run：`verified`；仅有 artifact：`partial`，不能单独通过 completion gate；
- required obligation 只要不是 `verified`，overlay 总体就是 `blocked`。

Overlay 只生成派生 JSON，不修改 task、manifest、log、status 或 artifact 真源。

验证结果：

```text
overlay_unit_tests       = verified
completion_gate_tests    = verified
live_manifest_mapping    = verified
```

当前 overlay 不根据 category、stem、命令文本或“看起来相似”的 artifact 自动推断 obligation 归属；调用方必须显式写入 `evidence_refs`。这避免把未声明的命令结果误当成证明。

### 13.2 research-wave 自动映射结果

已对 Phase 0C 计划生成带显式 run 引用的副本：

`target/codex-runs/harness-phase1/research-wave-0c-with-evidence.json`

并使用 Phase 0C 的真实 evidence manifest 生成 overlay：

`target/codex-runs/harness-phase1/research-wave-0c-overlay.json`

带 proof-status 的最终 overlay：

`target/codex-runs/harness-phase1/research-wave-0c-overlay-v2.json`

结果：

```text
all_children_return = verified
public_scope_only   = verified
join_review         = blocked
overall             = blocked
```

这里没有把 `native-subagent-wave-join.md` 的存在误当成 semantic join 通过。我们新增了专用的 `proof_status` artifact：它明确绑定 `join_review`，并由报告中的 `semantic_join: blocked` 派生；Overlay 因此记录为机器可读的 `blocked`，而不是解析普通 Markdown 或误判为 `verified`。

## 14. research-wave 可重复入口

已增加项目 OpenCode command：`.opencode/commands/research-wave.md`。

使用：

```text
/research-wave <要调查的问题>
```

该 command 固定当前已验证的最小工作流：root agent 一次并行调用两个 `terra-explorer` 和一个 `luna-extractor`，要求统一 ResultContract 字段，再区分 child execution、public scope、result contract 和 semantic join 四个门禁。它只做公开只读调查和 join，不写代码、不运行业务命令；实现任务必须在独立 writer task 中开始。

## 12. Phase 0C research-wave 当前结果

已执行三个真实只读 child：

```text
layout-owner           terra-explorer  succeeded
cross-facility-owner   terra-explorer  succeeded
evidence-entry         luna-extractor  succeeded
```

计划和 join artifact：

- `target/codex-runs/harness-phase0c/research-wave-0c.json`
- `target/codex-runs/harness-phase0c/reports/research-wave-0c-join.md`

三个 child 均遵守公开只读范围并返回结构化 claims、来源、unknowns 和 negative evidence。整体 semantic join 曾为 `blocked`，因为 cross-facility 调查发现 Global atom per-room 语义、StateConvert/FacilityLevel 扣回和 P2/P3 迁移状态存在未裁决冲突；这属于独立 domain side finding，不是 harness 改造前置条件。

这次阻塞是预期的 fail-closed 行为：child 成功不等于领域结论已验证。下一步应先做有界 `arknights-system-audit` 只读复审，不进入 writer 或 runtime 扩张。

## 13. Cross-facility 只读审计结果

审计报告：`target/codex-runs/harness-phase0c/reports/cross-facility-audit.md`。

审计确认 manufacture/trade 未按 scope 跳过 Global atom，Global `StateConvert` 存在潜在重复执行路径，`FacilityLevel` 类 room deduction 未覆盖，且当前文档对 per-room Global atom 语义和 P2/P3 状态存在冲突。

用户已裁决：Global atom 只由全局层执行一次，per-room 跳过，不再执行后扣回。因此 per-room 语义和 FacilityLevel 类扣回的两个问题已关闭。

仍待裁决：Global `StateConvert` 是否允许及其消耗语义、P2/P3 的迁移状态口径。这两项作为 `arknights-system-audit` side finding 跟踪；本 active-change 不授权修改转换语义或宣称 P2/P3 完成，但不阻塞 harness 的协议、adapter 和公开只读实验。

## 14. Runtime adapter probe

报告：`target/codex-runs/harness-phase0c/runtime-adapter-probe.json`。

通过用户提供的绝对路径确认 OpenCode CLI `1.18.19` 可用；当前 API/子进程 PATH 仍未发现它。项目 SDK import 未通过，Task 并行、child session resume/cancel 尚未验证。MiMo CLI 当前 PATH 未发现；Claude Code CLI 为 `2.1.181`，但 Claude Agent SDK 未安装；OpenAI Responses Multi-agent 未验证。探针未启动 runtime、未联网、未读取私有输入。探针支持通过 `HARNESS_OPENCODE_BIN` 和 `HARNESS_MIMO_BIN` 传入绝对路径。

OpenCode CLI 现在可以作为首个实际 adapter backend：已通过 `opencode run --agent terra-explorer
--format json` 运行一个只读 child 并返回带文件/行号的结果。多 child 并发、resume/cancel 和
SDK import 仍未验证；下一步先做三个 child 的受控并行 launch，不进入私有输入或写入路径。

补充验证：`terra-explorer` 不能直接作为 headless primary，OpenCode 会警告并回退默认 agent；
adapter 必须拒绝这种静默 fallback。改用内置 `plan` primary 后，三个 OpenCode CLI 子进程已并行
启动并全部返回 JSON 结果，报告为 `target/codex-runs/harness-phase0c/reports/opencode-plan-wave-join.md`。
因此 OpenCode CLI transport 和并行进程控制已验证，项目 subagent profile 透传、resume/cancel
和 SDK import 仍未验证。
