---
name: arknights-system-audit
description: Audit or repair ArknightsInfraCalc systems and conformance boundaries involving hard cores, producers, same-room/cross-station/in-base scope, required admission, Team/Shift binds, orchestration, rotation, or export. Use formal-audit only for a user-requested strict audit or conflicting canonical Markdown.
---

# Arknights System and Conformance Audit

Use this Skill when a domain invariant must be structurally guaranteed by System/Rule/Plan/search/schedule rather than accidentally produced by current ranking or room order.

## Select the Protocol

- **System conformance**: canonical Markdown is clear. Perform the four-item pre-write audit, then implement without two approval waits.
- **Formal audit**: the user requests strict itemized audit, or current canonical Markdown conflicts. Read `docs/SYSTEM_AUDIT_WORKFLOW.md` completely and obey its two waits.
- **Read-only audit**: report facts and counterexamples without acquiring write authority.

Do not load the formal workflow for ordinary system conformance. Use `docs/INDEX.md` only to locate the target canonical document and `docs/PROJECT_MAP.md` only when the code owner is unclear.

## Extract Invariants

Read the target canonical Markdown completely and inspect only its relevant registry/data rows. Cover as applicable:

- hard cores, tiers, minimum/all-available counts, optional producers;
- same-room, cross-station, in-base, recipe, capacity and mutual exclusion;
- Team membership, Shift work/rest, binding and closure/degradation;
- numeric scope, effect atoms and user-selected policy.

Code, data, fixtures, tests, and current top hits are implementation evidence, not rule sources.

## Trace and Challenge

Trace only as far as necessary through `select -> plan -> execute -> fill -> resolve -> rotation -> export`. Distinguish admission from activation/bind, eligibility from required placement, settlement from selection, and final output from a coincidental sample.

Use the counterexample that can change the owner: missing core, minimum members, competing candidate, reordered room, cross-station capacity, work/rest closure, or policy change. Do not require the complete counterexample catalog when the violation is already owner-local.

## Before Writing

Report:

1. confirmed domain invariants;
2. exact stage/file/type/function first allowing each invalid state;
3. the single post-fix owner;
4. old fallback, special case, comment, or test to delete/rewrite.

Declare one primary invariant, required/consumer/proof paths, and deferred findings. Never encode a repair through room id, Shift index, fixture, current top hit, or a bind/tag/priority/shortcut that cannot guarantee admission.

## Review and Prove

Use independent read-only exploration and invariant extraction by default when both code reality and domain documents are nontrivial. Freeze the responsibility boundary before one writer starts. For rebuilds, use an adversarial reviewer on the actual diff and any captured evidence.

Require invariant-level regression and a real `plan` or `layout team-rotation` entry for scheduling/export changes. Use `arknights-evidence` for formal audits and high-risk changes when durable command evidence is useful; it is not required for every audit command. Report the old illegal path, new owner, deleted conflicts, exact/heuristic impact if search space changed, tests, CLI result, and unresolved risks.

For Team/Shift, work/rest, mood, binds, capacity, producer shutdown, or MAA changes, also request `rotation-maa-reviewer` on the final diff. The proof must include the selected mode/profile, per-shift membership and state assertions, a legal and illegal bind/rest witness, and inspection of the generated MAA artifact. Do not treat a green unit test or an unchanged top schedule as conformance proof.

If the audit owns an active TODO, plan, or change package, apply
`../_shared/CHANGE_LIFECYCLE.md` after the invariant is proven. Formal
audit waits govern semantic approval; they do not waive current-document and
archive closure.
