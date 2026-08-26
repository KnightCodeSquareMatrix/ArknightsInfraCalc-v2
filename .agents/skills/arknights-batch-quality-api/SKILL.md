---
name: arknights-batch-quality-api
description: Batch Check updated Arknights solver bundles through the Quality HTTP API. Use after solver updates or when asked to batch solve representative/high-owned Boxes, poll compact status, report exact per-Box errors, or provide a downloadable result link.
---

# Arknights Batch Quality API

Use the Check service as the only source of batch inputs and results. This workflow is optimized for fast agent feedback without loading large solver results into context.

## Hard Rules

1. Use HTTP API calls only for layouts, Box bundles, job state, errors, and results.
2. Do not read local layout files, Box files, Check storage, `serve-response.json`, or downloaded result files.
3. Do not call the download URL yourself. Give the URL to the user or downstream automation.
4. Do not print full plan, profile, MAA, training advice, stdout, or response JSON into chat.
5. Use the compact summary endpoint for polling and error diagnosis.
6. There is no authentication. Do not invent or request a token.

## API Base

Use `QUALITY_CHECK_API_URL` when provided; otherwise use the local default:

```bash
API_BASE="${QUALITY_CHECK_API_URL:-http://127.0.0.1:5174}"
```

## 1. Discover Options

Call:

```bash
curl -fsS "$API_BASE/api/quality/status"
```

Read only these fields:

- `data.runner.ready`: must be `true`.
- `data.activeJobId`: must be `null` before creating another batch.
- `data.bundles[]`: choose the requested solver bundle. When checking an updated solver, match the intended Git commit to `gitCommit`; do not silently use another bundle.
- `data.options.layoutPresets`: allowed preset names.
- `data.options.boxSources`: allowed Box bundles.
- `data.options.rotations`: allowed rotation profiles.

If the target solver commit is not present, stop and say that the Check service has not installed that solver bundle yet.

## 2. Create A Batch

Submit the same configurable values as the Check page, plus the invariant debug gate:

```bash
curl -fsS -X POST "$API_BASE/api/quality/production-boxes" \
  -H 'Content-Type: application/json' \
  --data-binary "$(jq -nc \
    --arg bundleId "$BUNDLE_ID" \
    --arg layoutPreset "$LAYOUT_PRESET" \
    --arg source "$BOX_SOURCE" \
    --arg rotation "$ROTATION" \
    --argjson top "$TOP" \
    --argjson assertInvariants "$ASSERT_INVARIANTS" \
    '{bundleId:$bundleId,layoutPreset:$layoutPreset,source:$source,rotation:$rotation,top:$top,assertInvariants:$assertInvariants}')"
```

Parameters:

- `BUNDLE_ID`: exact ID from status.
- `LAYOUT_PRESET`: `243`, `153`, `333`, `252`, or `342` as advertised by status.
- `BOX_SOURCE`: normally `representative` or `high_owned`; `production` is also supported when configured.
- `ROTATION`: exact advertised profile.
- `TOP`: integer 1 through 100.
- `ASSERT_INVARIANTS`: boolean. Use `true` for solver/search, Plan, Team/Shift, rotation, MAA/export, cross-facility, or invariant debugging; otherwise use the user's choice, defaulting to `false`.

Store `data.jobId`, `data.summaryUrl`, and `data.downloadUrl`. Resolve returned relative URLs against `API_BASE`.

## 3. Poll Compact Status

Poll every two seconds:

```bash
curl -fsS "$API_BASE$SUMMARY_URL"
```

Stop when `data.status` is `completed`, `failed`, `paused`, or `cancelled`. Do not poll the case detail endpoint and do not fetch the download URL.

## 4. Report The Result

For a completed batch, report only:

```text
Batch completed: <completed>/<caseCount> Boxes.
Layout: <layoutPreset>; rotation: <rotation>; assert-invariants: <true|false>.
Download: <API_BASE><downloadUrl>
```

Do not summarize efficiencies or assignments unless the user explicitly asks for analysis after downloading the artifact themselves.

For any failed, paused, or cancelled batch, use `data.errors[]` and report every entry in this exact information order:

```text
Box <box> failed while running layout <layout>, rotation <rotation>, assert-invariants=<true|false>: [<code>/<stage>] <message>
```

If `stage` is null, omit only the stage. Never replace the actual message with a generic statement. End with:

```text
Download: <API_BASE><downloadUrl>
```

## 5. HTTP Or Creation Errors

The internal API exposes concrete diagnostics. On a non-2xx response, report `error.message` verbatim. Common examples are runner heartbeat expiry, another active job ID, missing Box source configuration, unsupported `plan.compute` schema v4, solver failure, or an `assert-invariants ...` diagnostic.

Do not retry configuration, capability, invariant, or solver errors. Retry only transient connection failures, at most three times with a two-second delay.
