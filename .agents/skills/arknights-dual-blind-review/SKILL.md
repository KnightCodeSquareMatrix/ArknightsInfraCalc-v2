---
name: arknights-dual-blind-review
description: Compare independent Terra and Sol reviews of a frozen ArknightsInfraCalc high-risk diff, invariant assertion output, and evidence packet. Use only for an explicit experimental review comparison after implementation and assertions are frozen; do not use it to replace required domain reviewers or batch acceptance.
---

# Dual Blind Review

Run this experiment only after the changed paths, canonical inputs, invariant assertion output, and evidence manifest are frozen. It is a model-routing evaluation surface, not an acceptance gate.

## Freeze The Packet

Create one task-local packet containing only:

```text
GOAL:
USER_DECISIONS:
CANONICAL_PATHS:
GUARANTEE_CLAIM:
CHANGED_PATHS:
DIFF_PATH_OR_COMMIT:
ASSERTION_COMMAND_AND_OUTPUT:
EVIDENCE_MANIFEST:
BATCH_RESULT: optional; only if already run
EXCLUSIONS:
```

Do not include the primary agent's root-cause analysis, design rationale, expected verdict, prior reviewer output, or another agent's conclusion. Point to files and artifacts instead of pasting unrelated history. Stop as `INCONCLUSIVE` if the canonical input, diff, or assertion evidence is absent.

## Dispatch

1. Spawn `terra-blind-reviewer` and `sol-blind-reviewer` in the same batch with `fork_turns="none"`.
2. Give both reviewers the identical frozen packet and no parent transcript.
3. Wait for both results. Do not steer either reviewer with the other result.
4. Preserve their raw reports as task-local evidence artifacts before comparing them.

## Compare

Compare verdict, blocking findings, invariant/guarantee classification, cited evidence, minimal illegal witness, and unverified gaps.

```text
DUAL_BLIND_REVIEW: AGREEMENT | MATERIAL_DISAGREEMENT | INCONCLUSIVE
TERRA_VERDICT:
SOL_VERDICT:
SHARED_FINDINGS:
TERRA_ONLY_FINDINGS:
SOL_ONLY_FINDINGS:
EVIDENCE_DISPUTES:
NEXT_ACTION:
```

`APPROVED` by either reviewer does not prove delivery. A material disagreement, missing evidence, or either `REVISE`/`INCONCLUSIVE` remains blocked until the primary agent resolves it against canonical sources and reruns affected evidence.

## Boundaries

- Keep the existing domain-specific Sol reviewer requirement and `arknights-invariant-gate` / batch acceptance workflow unchanged.
- Do not alter source tests or fixtures for this experiment.
- Record cache/token/latency observations separately from review quality. A cheaper reviewer is not accepted as equivalent without repeated real-task evidence.
