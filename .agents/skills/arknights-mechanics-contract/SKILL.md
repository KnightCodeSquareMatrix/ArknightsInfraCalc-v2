---
name: arknights-mechanics-contract
description: Verify ArknightsInfraCalc mechanics and runtime data contracts for skills, efficiency, drones, power, mood, orders, unit output, skill_table.json, or operator_instances.json. Use alongside arknights-maintenance when a change depends on source semantics, formulas, or source-to-runtime mapping.
---

# Arknights Mechanics Contract

This is an additive contract protocol. It proves how an approved game mechanic becomes runtime data and settlement behavior; it does not decide candidate selection, Plan admission, rotation construction, or deployment.

## Establish the Contract

1. Read repository `AGENTS.md`, then fully read the canonical Markdown for the mechanic. Use `docs/INDEX.md` only to locate the document.
2. Record the canonical rule, units, scope, timing, prerequisites, exclusions, and whether it is exact, probabilistic, or unsupported.
3. Trace the source row through registry/data loading, L1 interpretation, L2 settlement, and L3 shortcut or output only as far as the task requires. `data/` carries runtime values but cannot decide disputed semantics.
4. State the first source-to-runtime mapping or settlement owner that permits the wrong result. Do not repair an output formatter, fixture, or current top result when an earlier owner is wrong.

## Prove the Mapping

For every changed mechanic, report:

- canonical document and exact data rows;
- code owner, type, function, and affected output field;
- formula decomposition with units and scope;
- prerequisites, zero/empty case, boundary case, and one adjacent counterexample;
- whether the result is exact, a policy, a heuristic, an approximation, or unsupported.

Add the smallest fixture or regression that exercises the mapping and its adjacent exclusion. Use `arknights-evidence` when command, CLI, generated-output, or data-audit proof needs durable records. A green snapshot does not prove a formula when its inputs do not isolate the changed term.

## Stop Conditions

Stop after one canonical mapping owner guarantees the approved rule, conflicting compensations are removed or deferred, and the selected proof distinguishes the old and new behavior. If canonical documents conflict, stop semantic writing and request a user ruling.

Return: contract summary, source rows, formula, runtime owner, proof cases, changed paths, unresolved semantics, and evidence locations.
