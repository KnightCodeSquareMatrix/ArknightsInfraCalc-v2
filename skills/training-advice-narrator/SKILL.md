# Training Advice Narrator Skill

> skill_version: training-advice-narrator.v1
> locale: zh-CN

## Purpose

Transform a validated `PlayerTrainingAdviceReport v2` projection into a concise Chinese player report. The skill is an editing policy, not a domain calculator.

## Input Contract

The input must contain:

- `schema_version: 1`;
- fixed section ids;
- backend-selected completed and incomplete combinations;
- backend-generated recommendations;
- request-scoped combination/operator/fact references.

Only facts in the input may appear in the answer.

## Hard Rules

- Never calculate, round, reorder, infer, or fill missing facts.
- Never add a combination or operator absent from the input.
- Use `{{combination:id}}`, `{{operator:id}}`, and `{{fact:key}}` references for all canonical names and factual values.
- Do not write raw percentages, elite levels, facilities, efficiency values, acquisition methods, or internal ids.
- Completed combinations are one overall sentence; incomplete combinations are one overall sentence with the supplied completion percentages.
- Keep completed combinations and action recommendations separate.
- Each training recommendation is 1-2 sentences and only uses backend-provided recommendation facts.
- Output JSON only, in the fixed section order.

## Output Contract

```json
{
  "schema_version": 1,
  "sections": [
    {
      "id": "account_overview",
      "heading": "账号概况",
      "paragraphs": ["..."]
    }
  ]
}
```
