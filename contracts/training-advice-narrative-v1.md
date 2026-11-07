# `training_advice.narrative` v1

本契约定义 BFF 与 Python narrator adapter 的边界，不修改 `plan.compute` 的 `training_advice` 结构化契约。

## 请求

请求必须携带已经成功生成的 `PlayerTrainingAdviceReport` v2。BFF 应在进入 adapter 前完成用户鉴权、请求体限制和内容 HMAC 计算。

```json
{
  "schema_version": 1,
  "locale": "zh-CN",
  "player_report": { "schema_version": 2 },
  "mapping_path": "data/canonical_mapping.json"
}
```

adapter 的核心调用顺序为：

```python
mapping = CanonicalMapping.load(mapping_path)
facts = project_report(player_report, mapping)
result = Narrator().generate(facts)
```

`player_report` 中的完成率、状态、推荐顺序、目标练度和设施是唯一事实来源。adapter 不重新计算或排序。

BFF 可直接复用 `arknights_narrator.cache` 的 `content_hmac`、`RedisKeys` 和 `admit`。这些函数只负责服务端内容身份、key 隔离和“ready → SET NX → accepted/processing/fallback”决策，不替代 Redis 客户端或任务队列。

内容 HMAC 必须覆盖规范化 report、locale、skill/prompt 版本、mapping hash 和 model。客户端不得提供 hash。Redis key 使用以下独立前缀：

```text
arknights:training_advice:narrative:v1:ready:{content_hmac}
arknights:training_advice:narrative:v1:fallback:{content_hmac}
arknights:training_advice:narrative:v1:lock:{content_hmac}
arknights:training_advice:narrative:v1:task:{task_id}
```

锁 value 必须包含 owner/fencing token。`admit(...)=ACCEPTED` 返回的 `lock_token` 是发布结果、续租和 compare-and-delete 释放锁的唯一凭据；BFF 不得重建或猜测该 token。发布结果和释放锁使用 adapter 暴露的 Lua 脚本。BFF 仍需将锁、task state 和入队放入同一 Redis slot 的事务边界。

## 响应

```json
{
  "status": "ready",
  "narrative": {
    "schema_version": 1,
    "sections": []
  },
  "structured_report": { "schema_version": 2 },
  "fallback_reason": null
}
```

`status` 为 `ready` 时，`narrative` 已经通过 placeholder、mapping 和长度校验。LLM 未配置、超时、输出不合规、mapping 不一致或 provider 失败时，`status` 为 `fallback`，并返回确定性报告。`fallback_reason` 只用于服务端日志和调试，不应暴露供应商错误细节给玩家。

## BFF 异步行为

公共接口建议由 BFF 提供：

```text
POST /training_advice.narrative
GET  /training_advice.narrative/{task_id}
```

- 先查询 `ready` cache；
- 未命中后对内容 HMAC 对应的 lock 使用 `SET NX EX`；
- 未抢锁者返回 `202 processing`，不等待、不重复入队；
- Redis Connection Refused/Timeout 时跳过任务和 LLM；POST 若带可验证 report 则直接 fallback；
- `plan.compute` 不等待 narrator，也不依赖 narrator 成功。

详见 [自然语言推荐层设计](../docs/产品与训练/自然语言推荐层设计.md)。
