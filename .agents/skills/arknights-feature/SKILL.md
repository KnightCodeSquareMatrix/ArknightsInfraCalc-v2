---
name: arknights-feature
description: Design and implement new ArknightsInfraCalc product capabilities from canonical domain rules. Use for new operators/mechanics, commands, APIs, website-facing outputs, scheduling modes, restored TODO work, or any intentional expansion of feasible behavior or user policy.
---

# Arknights Feature Development

Deliver one user-visible vertical capability without disguising it as a bug fix or using it to reopen unrelated plans.

## Establish the Contract

1. Read repository `AGENTS.md`.
2. State the user scenario, observable output, canonical domain rule, success evidence, and explicit non-goals.
3. Read the target canonical domain document completely. Use `docs/INDEX.md` only when its location is unknown. For **练卡推荐 / advice 玩家语义**, start at `docs/练卡推荐相关/README.md` (not the archived v2 机器诊断合同).
4. Inspect existing extension points and callers directly; use the relevant `docs/PROJECT_MAP.md` section only when ownership is unclear.
5. If restoring a TODO, treat it as an active change: reconcile it with current
   docs/code before implementation, then apply
   `../_shared/CHANGE_LIFECYCLE.md` through closure.

If domain semantics are missing or conflicting, pause in semantic audit and obtain a ruling before writing.

## Choose the Change Shape

- **Local extension**: existing schema/lifecycle expresses the capability. Add the smallest complete vertical slice.
- **Preparatory rebuild**: the capability cannot be expressed without changing a shared owner. Separate behavior-preserving structure work from the new behavior in reviewable steps; do not generalize unrelated domains.
- **Independent quality finding**: record and route to `arknights-quality` unless it blocks this feature's contract.

For systems, required admission, cross-facility scope, or rotation binding, use `arknights-system-audit`. For commands/artifacts, use `arknights-evidence`.

## Build a Vertical Slice

Cover the required lifecycle only: domain model/data, solver/Plan, public API/CLI, output/export, documentation, regression, and real entry as applicable. Keep CLI/export free of mechanism and policy decisions.

When candidate space or objectives change, name the hard constraint, safe reduction, heuristic, policy, or approximation and define the result guarantee. Do not turn a common top hit into a hard rule.

Use read-only explorers for extension points/callers and extractors for multi-document specs. The main Agent freezes the contract before one writer starts; use an adversarial reviewer for product-semantic or multi-module changes.

Stop when the user scenario works through the real entry, shared owners remain singular, old transitional paths are removed, non-goals remain untouched, and risk-matched evidence is complete.

For tracked features, completion also requires merging confirmed behavior into
the current canonical owner, extracting durable decisions when needed,
splitting remaining work, archiving the change package, and closing indexes and
references. Documentation is part of the vertical slice, not a follow-up.
