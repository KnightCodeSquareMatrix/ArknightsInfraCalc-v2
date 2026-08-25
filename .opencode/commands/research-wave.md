---
description: Run a public read-only parallel research wave and produce a fail-closed join.
agent: build
---

Run a Phase 0C research wave for the user's question. The output is an
`agent-harness-v0` envelope whose `task.research_wave` field is the only
`research-wave-v0` specialization. Never emit or accept a top-level
`research-wave-v0` TaskRecord.

> $ARGUMENTS

Follow this workflow exactly:

1. Read the applicable `AGENTS.md` files and the relevant canonical Markdown before dispatching children. Do not modify files, run shell commands, or use private operbox/layout/assignment inputs.
2. Dispatch these children in one parallel batch using the native `task` tool:
   - `terra-explorer`: investigate the primary code/domain owner and call chain for the question.
   - `terra-explorer`: investigate the main counterexample, cross-boundary behavior, or competing owner.
   - `luna-extractor`: extract relevant canonical rules, existing evidence entry points, stable fields, and missing items.
3. Every child must return:
   - `findings`
   - `claims`, with `fact`, `inference`, `unknown`, `unsupported`, `policy`, or `decision`
   - `sources`, including file paths and line numbers
   - `unknowns`
   - `negative_evidence`
   - `canonical_conflicts`
   - `reviewer_required`
4. After all children return, join their results. Separate these statuses:
   - `execution`: whether the child completed;
   - `scope`: whether the child stayed public and read-only;
   - `result_contract`: whether the required fields are present;
   - `semantic_join`: whether domain claims are sufficiently reviewed.
5. Apply the fail-closed gate:
   - a child failure is `failed`;
   - missing required output is `blocked`;
   - any canonical or user-decision conflict is `blocked` and must not be resolved by inference;
   - child execution success must never be presented as semantic verification.
6. Return one concise join report in this shape:

```text
Research wave: <question>

Children:
- <child>: succeeded|failed, scope=verified|blocked, result_contract=verified|blocked

Findings:
- <claim with source>

Unknowns and conflicts:
- <item>

Gate:
execution = verified|blocked
scope = verified|blocked
result_contract = verified|blocked
semantic_join = verified|blocked
overall = verified|blocked

Next action:
- <one concrete next investigation or implementation boundary>
```

Do not edit code or documentation from this command. Do not run tests or business CLI commands. If the user asks for implementation, stop after the join and return the proposed owner and proof obligations for a separate writer task.
