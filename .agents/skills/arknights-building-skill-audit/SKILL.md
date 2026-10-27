---
name: arknights-building-skill-audit
description: Audit and repair ArknightsInfraCalc operator building-skill unlocks and facility bindings against ArknightsGameResource. Use for tier_0/tier_up, E0/E1/E2 unlock, skill order, operator_instances, building_data, or suspected early/missing facility skills; preserve the project's intentional two-state runtime model and require provenance before judging custom local buff IDs.
---

# Building Skill Unlock Audit

Use this Skill when a building skill may be enabled at the wrong operator
progression stage, omitted from a facility, copied into the wrong facility, or
merged in the wrong order.

## Contract

The upstream fact source is:

```text
ArknightsGameResource/gamedata/excel/building_data.json
  chars[char_id].buffChar[].buffData[].buffId
  chars[char_id].buffChar[].buffData[].cond.phase
  chars[char_id].buffChar[].buffData[].cond.level
```

The project runtime intentionally has two effective states:

```text
tier_0  = upstream phase 0
tier_up for 3/4-star operators = upstream phase 1
tier_up for 5/6-star operators = upstream phase 2
```

Do not replace this with a three-state runtime model unless the user explicitly
authorizes that architectural change. `tier_up` is a runtime state, not a
literal synonym for Elite 2.

## Ownership

- Upstream `building_data.json`: unlock phase and facility facts.
- `data/operator_instances.json`: local operator-to-facility binding and local
  buff IDs.
- `data/skill_table.json`: local EffectAtom formulas; it does not decide unlock
  phases by itself.
- `data/building_skill_provenance.json`: accepted local-to-upstream ID aliases.
- `scripts/audit_building_skill_unlocks.py`: deterministic comparison and report.
- `scripts/sync_building_skills.py`: upstream metadata extraction; it does not
  invent EffectAtoms or rewrite local semantics.

Do not infer unlock stages from skill names, buff ID prefixes, rarity alone, or
the current top assignment.

## Finding Classes

The audit emits separate classes:

- `future_unlock_in_tier_0`: a mapped upstream phase 1/2 skill is active in
  `tier_0`.
- `unexpected_or_stale_tier_up`: a mapped `tier_up` binding contains an
  earlier, wrong, or stale phase skill after stepwise resolution.
- `missing_phase_0`: an upstream phase 0 skill has no local effective binding.
- `missing_tier_up`: an upstream skill expected in the operator's effective
  `tier_up` state is absent.
- `wrong_facility_binding`: a known buff is attached to a different facility.
- `unmapped_local_binding`: a local custom buff ID has no accepted provenance;
  this is not proof of a bug.

Only the first, second, and fourth classes with reliable provenance are direct
stage defects. `missing_phase_0` and `missing_tier_up` are completeness
findings until the local facility scope is confirmed. `unmapped_local_binding`
requires manual semantic review.

## Workflow

1. Read `AGENTS.md` and the relevant canonical mechanism document. Record the
   operator, layout, operbox, expected stage, and current output.
2. Run the script unit tests:

   ```bash
   env PYTHONPATH=scripts python3 -m unittest \
     scripts.tests.test_audit_building_skill_unlocks
   ```

3. Run the full upstream audit and retain the JSON report:

   ```bash
   python3 scripts/audit_building_skill_unlocks.py \
     --json-out target/codex-runs/building-skill-unlock-audit/report.json
   ```

   For a focused reproducer:

   ```bash
   python3 scripts/audit_building_skill_unlocks.py \
     --operator 夜烟 --json-out target/codex-runs/building-skill-unlock-audit/night-smoke.json
   ```

4. Compare local and upstream IDs before changing data. Add an alias to
   `data/building_skill_provenance.json` only after the local EffectAtoms and
   upstream description are semantically equivalent. Keep uncertain aliases
   as `pending`; the audit must continue reporting them as unmapped.
5. Repair only confirmed `operator_instances.json` bindings. Preserve the
   existing `stepwise` semantics: it merges same-stem skills within the
   effective runtime state; it must not cause a phase 2 skill to survive when
   the upstream phase 2 skill replaces phase 0.
6. Add a focused regression for every repaired operator. At minimum assert
   `tier_0` and `tier_up` resolved buff IDs, facility membership, and the
   relevant rarity/progression mapping.
7. Re-run the focused real entry. For cross-facility changes, use the actual
   `layout test` or `layout team-rotation` command and verify the final
   assignment, not only the independent candidate benchmark.
8. Record evidence with `scripts/codex/run_evidence.sh` when durable logs are needed; a nonzero full audit is expected
   while unresolved completeness or pending-alias findings remain.

## Guardrails

- Never automatically rewrite all 220-style findings. Most are incomplete
  local bindings or custom IDs, not proven stage defects.
- Never accept a local alias from a matching English/Chinese skill name alone.
- Never move a skill between facilities without checking the upstream room
  type and the local EffectAtom scope.
- Never turn upstream phase data into a new runtime tier without an explicit
  architecture decision.
- Keep generated reports and raw upstream evidence outside committed runtime
  data unless the repository explicitly treats them as generated references.

## Stop Conditions

Stop after:

- all confirmed phase mismatches for the requested scope are zero;
- accepted aliases are recorded with provenance;
- unresolved custom IDs and completeness findings are listed separately;
- focused runtime and regression tests pass;
- no unreviewed bulk rewrite remains.

The normal residual state may be:

```text
phase mismatches = 0
unmapped aliases > 0
completeness findings > 0
```

That state means stage safety is proven for the audited scope; it does not
claim that every facility skill is modeled.
