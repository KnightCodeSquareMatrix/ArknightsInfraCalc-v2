---
name: arknights-invariant-gate
description: Add and run final-state Rust invariant assertions plus Python batch acceptance over persistent ArknightsInfraCalc operbox bundles after high-risk solver, plan, rotation, export, or mechanics changes.
---

# Arknights Final-State Invariant Gate

Use this Skill after a large or high-risk change that can produce a plausible-looking but invalid final assignment. This is an additive quality protocol; domain canonical documents still define the invariant.

## Required Shape

1. State the invariant in terms of the final `TeamRotationReport.shifts[].assignment`, not text output, MAA JSON, a fixture name, or a current top hit.
2. Add the invariant to the Rust `--assert-invariants` gate after scheduling and before profile/MAA output. Each registered check must have a stable diagnostic name.
3. Reconstruct the check from the final assignment with the same runtime inputs used by settlement: actual operbox progress, instances, skill table, and resolved layout context. Do not reimplement mechanism rules in Python.
4. Do not add, modify, or run source-code tests. A runtime assertion returns `Result` failure; validation belongs to the Rust gate and Python batch runner.
5. Update or create a Python batch runner that invokes the Rust gate with `--assert-invariants` over persistent operbox bundles. Python reports per-box solver failures separately from invariant-gate failures.

## Persistent Batch Inputs

- Representative boxes: `data/fixtures/box-bundles/collected/collected_operboxes/`.
- High-owned boxes: `data/fixtures/box-bundles/high-owned/`.
- Do not read transient external archives or parse human-readable CLI/MAA output to decide an invariant. If a supplied archive is needed, extract it once into `data/fixtures/box-bundles/` before batch validation.

## Evidence and Review

1. Run one real `plan` with `--assert-invariants` and both batch groups through `scripts/codex/run_evidence.sh`; do not run source-code tests.
2. Report separately: checked count, invariant failures, solver infeasible/no-solution failures, parse/input failures, and timeouts.
3. For solver/search, Plan, Team/Shift, rotation, MAA, export, or cross-facility changes, request `sol-reviewer` on the actual assertion diff and evidence. Revise until the reviewer returns `PASS`.
4. Never claim a batch green result proves global optimality. It proves the named final-state invariant for the completed runs only.

## Stop Condition

Stop when one Rust final-state gate owns the invariant, the Python runner calls that gate rather than duplicating semantics, focused tests and batch evidence distinguish solver failure from invariant failure, and Sol review passes.
