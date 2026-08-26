---
name: arknights-quality
description: Plan and execute independently authorized ArknightsInfraCalc architecture, performance, workflow, technical-debt, documentation-governance, or solver-assurance improvements. Use when recurring evidence justifies a quality task; do not auto-expand a local bug merely because nearby code could be cleaner.
---

# Arknights Quality Improvement

Treat quality as an independent task with an economic and correctness case, not as opportunistic cleanup attached to a bug.

## Justify and Bound

1. Read repository `AGENTS.md` and state the recurring friction, risk, or capability cost.
2. Cite concrete evidence: repeated incidents, multiple owners, recurring document drift, persistent validation ambiguity, measured performance limits, or an unprovable solver guarantee.
3. Define one quality invariant, baseline, expected benefit, migration boundary, proof, stop condition, and non-goals.
4. Read only the relevant current architecture/quality sections. Use `docs/PROJECT_MAP.md` when owner facts are unclear and `docs/INDEX.md` only to locate affected canonical documents.

Do not start a quality rebuild solely because code is large, duplicated, or inelegant. Do not let an unrelated bug silently authorize it.

## Design the Migration

- Identify the new single owner and every old conflicting path to remove.
- Separate behavior-preserving migration from deliberate behavior change.
- Require a second real semantic pattern or clearer proof/ownership benefit before introducing a generic abstraction; allow legitimate named domain mechanisms.
- Keep one writer after the main Agent freezes the design. Use independent exploration/extraction before writing and an adversarial reviewer on the actual diff.

For solver/search work, read the guarantee and risk sections of `docs/QUALITY_AND_AUDIT.md`. Classify hard constraints, reductions, heuristics, policies, approximations, result status, oracle/differential evidence, and performance/fallback behavior.

If the quality migration changes System/Rule/Plan admission, cross-facility scope, Team/Shift binds, rotation, or export fidelity, also use `arknights-system-audit` as the conformance protocol. This Skill remains the primary owner of the independently authorized quality goal and scope.

For Agent/workflow changes, preserve progressive disclosure: root rules route, Skills execute, canonical docs define semantics, deterministic scripts enforce mechanical facts. Avoid adding new memory or roles without a demonstrated missing owner.

For documentation governance, use `../_shared/CHANGE_LIFECYCLE.md`; the role,
state, ownership, and closure semantics are owned only by
`docs/文档生命周期.md`. Do not restate or weaken that lifecycle in this Skill.

## Complete

For large quality migrations, use `arknights-evidence` to capture baseline and current facts. Prove migration completion, delete old paths, compare behavior/performance/validation as scoped, update current docs, and defer unrelated discoveries.

When the quality task owns a tracked plan or documentation migration, closure
must also prove lifecycle state, index coverage, and repository references. Do
not leave completed or superseded plans in the active tree with banner-only
updates.

Report why this was an independent quality task, old maintenance cost or correctness risk, new owner, deleted conflicts, measured result, unverified risks, and the next real task that can now be done more safely or cheaply.
