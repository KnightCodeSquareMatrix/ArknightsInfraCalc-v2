---
name: arknights-maintenance
description: Reproduce and repair ArknightsInfraCalc bugs with a change sized to the real responsibility boundary. Use for wrong results, CLI, data, solver, layout, schedule, export, regressions, or "run it once" requests; allow small owner-local fixes and escalate structural system issues to arknights-system-audit.
---

# Arknights Debug and Repair

Keep one observable invariant in scope. A small change is correct when the existing owner can already express the rule; rebuild only when the owner is missing, split, or structurally unable to prevent the invalid state.

## Route Progressively

1. Read repository `AGENTS.md` and record the actual command/input, expected result, and current result.
2. Read the canonical domain document for the affected rule. Use `docs/INDEX.md` only when that document is unknown. **练卡推荐玩家语义** → `docs/练卡推荐相关/`；旧 `training_recommendations` 行为仅作实现对照，不得覆盖玩家 canonical。
3. Reproduce through the existing user entry. Read the relevant `docs/MAINTENANCE_MODE.md` section only when the reproduction or acceptance entry is unclear.
4. Inspect `docs/PROJECT_MAP.md` only when the command, owner, or lifecycle location cannot be found directly from code/search.
5. Use `arknights-evidence` when the change is large, high-risk, or needs durable reproducible records; ordinary local fixes may use direct commands.
6. If the repair owns an active TODO, plan, or change package, read and apply
   `../_shared/CHANGE_LIFECYCLE.md` before declaring it complete.

## Classify Before Writing

- **Local repair**: canonical semantics are clear, the correct owner exists, and the error is internal to it. Fix it directly; do not demand preparatory architecture work.
- **Conformance repair**: the model cannot express the invariant or multiple downstream paths compensate for it. State the invariant, first violating stage, new single owner, and conflict paths to delete. For systems, cross-facility, Plan admission, scope, or rotation binds, use `arknights-system-audit`.
- **Independent quality issue**: the current bug can be fixed in the existing owner and adjacent structure only affects future cost. Finish the bug, record the evidence-backed finding, and route it to `arknights-quality` rather than expanding this task.
- **Feature**: the request changes capability, feasible behavior, policy, output contract, or user mode. Route to `arknights-feature`.

If canonical Markdown conflicts, stop semantic writing and request a ruling. Code, tests, fixtures, and output cannot choose the rule.

## Implement and Stop

1. Change the earliest owner that permits the invalid state; keep normal domain paths intact.
2. Add a minimal reproducer plus adjacent counterexample. For candidate/search changes, test eligibility and exclusion, not only the top result.
3. Run the risk-matched real entry and focused regression. Use the evidence wrapper when the task needs durable records.
4. Stop when one owner guarantees the invariant, direct conflict paths are gone, changed paths are declared, and remaining discoveries are deferred.
5. For tracked work, close the lifecycle transaction: update current truth,
   split valid open work, archive the closed change, and prove index/reference
   integrity. A completed checklist left in the active tree is not complete.

Use a read-only explorer when the first illegal state or owner is unclear; use an extractor for complex logs/baselines; use an adversarial reviewer for medium/high-risk solver, schedule, system, or export changes. Skip delegation when no independent judgment axis exists.

Report root cause, why the old model allowed the state, the single fact owner, changed paths, deferred findings, docs impact, tests, real entry, any evidence captured, and unresolved risks.
