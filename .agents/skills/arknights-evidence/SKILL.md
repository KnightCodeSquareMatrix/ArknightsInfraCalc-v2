---
name: arknights-evidence
description: Record and audit reproducible ArknightsInfraCalc build, test, CLI, benchmark, format, structure, generated-artifact, task-scope, and solver-assurance evidence. Use whenever a conclusion depends on a command or artifact; never replace domain or optimality review with a script result.
---

# Arknights Evidence

Use repository evidence tools when a task needs durable, reproducible verification. They are recommended for large refactors, solver/search changes, system/rotation/export work, deployment, and formal audits; ordinary local fixes and documentation changes may use direct commands.

## Record Runs

1. Create task metadata from `scripts/codex/task_metadata.example.json`: one invariant, root layer, required/consumer/proof/deferred paths, side findings, and reviewer fields.
2. Run `scripts/codex/run_evidence.sh --task <slug> --category <category> --stem <name> --inputs '<inputs>' -- <command...>`.
3. Register generated profile, MAA, comparison, or report files with repeated `--artifact kind=path`.
4. Keep every rerun. Do not overwrite failures or use shared output names.

Use categories `build`, `targeted-test`, `full-suite`, `cli`, `performance`, `format`, and `structure`. Include actual layout, operbox, assignment, fixture, policy, output, baseline, seed, and time limit when relevant.

For tracked changes and documentation migrations, run the lifecycle adapter in
`../_shared/CHANGE_LIFECYCLE.md` and the checker required by
`docs/文档生命周期.md`. If no deterministic checker exists for a claimed
invariant, mark that part unverified rather than treating manual confidence as
machine proof.

## Match Evidence to Risk

- Owner-local data/logic fix: minimal reproducer, adjacent counterexample, and affected real entry.
- Hard constraint or eligibility: activation/rejection boundaries and final feasibility.
- Objective/tie-break: component and equivalent-optimum checks.
- Candidate generation, pruning, decomposition, Bake/cache, or performance: read the solver-assurance sections of `docs/QUALITY_AND_AUDIT.md`; record guarantee class, differential/oracle or fallback evidence, candidate/benchmark facts, and result status.
- Schedule/export: Team/Shift invariants plus real `plan`/MAA fidelity.

Do not impose every global category on a low-risk documentation or owner-local fix. Do not let a high-risk search-space change pass on one golden snapshot.

## Compare and Finish

For full suite, use `scripts/codex/compare_test_failures.py` on complete Cargo logs and compare exact failure-name sets.

Before completion run:

1. `scripts/codex/check_task_scope.py --manifest <manifest>`;
2. `scripts/codex/render_evidence.py --manifest <manifest> --output <report>`.

When a task closes or supersedes tracked work, inspect the final tree as well as
the diff and require the lifecycle hard check; scope scripts do
not replace that proof.

Inspect logs, status files, exit codes, artifacts, changed paths, deferred findings, and renderer “未跑” categories. An extractor may organize complex logs or failure sets, but the main Agent must judge domain semantics, exact/heuristic claims, and final completion.
