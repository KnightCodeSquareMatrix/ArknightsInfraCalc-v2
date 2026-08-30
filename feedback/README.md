# Feedback Tracking

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/MAINTENANCE_MODE.md；docs/文档生命周期.md
> 摘要：说明本地生产反馈 bundle 和 evidence ledger 边界

This directory stores production feedback bundles copied from deployed services. `sync_feedback.py`
keeps the existing batch flow: it sorts feedback by Beijing `savedAt`, then follows each
`diagnosticId` to the matching service `cli-runs` directory and imports its reproduction bundle.

Treat each feedback folder as raw evidence. Do not edit `issue.json`, `meta.json`, `debug-bundle.json`, or `operbox.json` while triaging. Add investigation state in [TRACKING.md](TRACKING.md) instead.

The imported 2026-06-27 to 2026-06-30 batch is closed as of 2026-07-03. [TRACKING.md](TRACKING.md) is now a closure audit and regression guard, not an open bug queue. Reopen a row only if the same symptom is reported again.

## Folder Shape

Each case normally contains:

| File | Meaning |
|------|---------|
| `issue.json` | User-visible complaint, selected room, note, and original online command |
| `meta.json` | Feedback id, saved time, source name, and bundle availability |
| `debug-bundle.json` | Online run context: layout, operbox summary, command, exit code, and result payload when available |
| `operbox.json` | The submitted operator box for local reproduction |
| `layout.json` | The submitted base layout for direct CLI investigation |
| `command.txt` | Recorded online CLI command; evidence only, never execute it locally |
| `serve-request.jsonl` | Original `infra-cli serve` request used by the recorded command |
| `result.json` | Original online result wrapper for comparison and diagnosis |

The import copies all ordinary files from the matching remote `cli-runs` directory. The six files
listed above are the required completeness check; `profile.json`, `maa.json`, request/response
snapshots, and stdout/stderr are retained as additional evidence when present. The import is
additive: later batch syncs refresh `issue.json` and `meta.json` but retain already imported
reproduction artifacts. A feedback entry without a matching or complete remote run bundle remains
an index-only case and is reported by the script.

## Batch Import

Run from the repository root:

```bash
python3 scripts/sync_feedback.py
```

The default source list is the beta, dev, and quality service feedback directory. To import one
service while checking a deployment, repeat `--remote-dir` only for the desired service.

## Current-runtime rerun

For a successful `version=beta-test-bundle-v2-next-serve` bundle with `method=plan` or
`method=plan.compute`, use the repository adapter instead of executing the recorded online command or paths:

```bash
cargo run -q -p infra-cli -- debug rerun \
  --bundle feedback/<date>/<case>/debug-bundle.json \
  --output-dir target/codex-runs/<task>/private-rerun \
  --pretty
```

This is a rerun against the current executable and resolved data catalogs, not proof of the original
online runtime: historical bundles do not contain its binary/data fingerprints. The command compares
stable rotation/assignment fields when the recorded response is available and marks the original
runtime as unknown. It never executes `command`, `savedFiles`, request paths, or user-controlled labels.

The output directory must not already exist. Extracted layout, operbox, assignments, and reports remain
private local evidence and must not be committed. Stdout contains only a sanitized hash/status summary;
the complete rotation, focused room/operator projection, source name, and recorded projection are written
only to the explicit private output directory. Explicit `--top` or `--rotation` overrides disable recorded
result comparison. Historical `plan.compute` snapshots may contain a `rarity=1` unknown-rarity sentinel;
the rerun adapter preserves their recorded `elite`/`level` facts and records the compatibility count.

## Status Values

| Status | Meaning |
|--------|---------|
| `intake` | Imported and indexed, not reproduced locally yet |
| `reproduced` | Local command reproduces the reported symptom |
| `localized` | Suspected layer is confirmed |
| `fixing` | A minimal fix is in progress |
| `regressed` | A fixture, CSV anchor, or test now protects the case |
| `closed` | Fix and regression are verified |
| `blocked` | Missing input, unclear expectation, or cannot reproduce |
| `duplicate` | Tracked through another case id |
| `duplicate-covered` | Closed through another feedback case in the same root-cause family |

## Triage Rule

For each case:

1. Record the local reproduction command before changing code.
2. Localize to one layer: CLI, layout, schedule, search, solver, mechanism, data, or output.
3. Fix only that layer.
4. Add the smallest regression that would fail before the fix.
5. Update [TRACKING.md](TRACKING.md) with status, fixed commit, and verification command.

If a case uses private account data, keep the raw bundle local unless the user explicitly asks to commit it.
